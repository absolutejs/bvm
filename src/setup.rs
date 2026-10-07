//! Putting bvm's shims first on PATH, and the `bvm` shell function that lets
//! `bvm use` change the current shell (a program cannot change its parent's
//! environment; nvm solves this the same way).

use crate::paths;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

const MARKER: &str = "# bvm (Bun version manager)";

/// Shell code for `eval "$(bvm env)"`: PATH, then a `bvm` function whose `use`
/// runs in this shell.
pub fn posix_env() -> Result<String> {
    let root = paths::root()?;
    Ok(format!(
        r#"export BVM_DIR="{root}"
case ":$PATH:" in *":$BVM_DIR/bin:"*) ;; *) export PATH="$BVM_DIR/bin:$PATH" ;; esac
bvm() {{
  if [ "$1" = use ]; then
    eval "$(command bvm use --print-env "$2")"
  else
    command bvm "$@"
  fi
}}
"#,
        root = root.display()
    ))
}

pub fn powershell_env() -> Result<String> {
    let root = paths::root()?;
    Ok(format!(
        r#"$env:BVM_DIR = "{root}"
if (-not ($env:Path -split ';' -contains "$env:BVM_DIR\bin")) {{ $env:Path = "$env:BVM_DIR\bin;$env:Path" }}
function bvm {{
  if ($args[0] -eq 'use') {{ Invoke-Expression (& (Get-Command bvm -CommandType Application | Select-Object -First 1).Source use --print-env --shell powershell $args[1]) }}
  else {{ & (Get-Command bvm -CommandType Application | Select-Object -First 1).Source @args }}
}}
"#,
        root = root.display()
    ))
}

fn append_once(file: &Path, line: &str) -> Result<bool> {
    let existing = fs::read_to_string(file).unwrap_or_default();
    if existing.contains(MARKER) {
        return Ok(false);
    }
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(file, format!("{existing}{separator}\n{MARKER}\n{line}\n"))?;
    Ok(true)
}

/// The shell startup files to set up: the user's own shell's file (created if
/// missing: a fresh macOS account has no `.zshrc`), plus any other bash or zsh
/// file that already exists. macOS terminals start login shells, which read
/// `.bash_profile` rather than `.bashrc`.
#[cfg(unix)]
fn posix_startup_files(home: &Path, shell: &str) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut add = |name: &str, create: bool| {
        let file = home.join(name);
        if (create || file.exists()) && !files.contains(&file) {
            files.push(file);
        }
    };
    add(".bashrc", shell == "bash" || shell.is_empty());
    add(".zshrc", shell == "zsh");
    add(
        ".bash_profile",
        shell == "bash" && cfg!(target_os = "macos"),
    );
    files
}

/// Puts bvm on PATH for new shells: the user's shell startup files on Unix;
/// on Windows, the user PATH (announced to running programs, as rustup does)
/// and the PowerShell profile.
pub fn setup() -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    let bvm = paths::bin_dir()?.join(format!("bvm{}", crate::platform::exe_suffix()));
    #[cfg(unix)]
    {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let shell = std::env::var("SHELL")
            .ok()
            .and_then(|path| {
                Path::new(&path)
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let eval = format!(r#"eval "$("{}" env)""#, bvm.display());
        for file in posix_startup_files(&home, &shell) {
            if append_once(&file, &eval)? {
                changed.push(file);
            }
        }
        let fish = home.join(".config/fish/conf.d/bvm.fish");
        if (shell == "fish" || home.join(".config/fish").exists())
            && append_once(
                &fish,
                &format!("\"{}\" env --shell fish | source", bvm.display()),
            )?
        {
            changed.push(fish);
        }
    }
    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
        let environment = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags("Environment", KEY_READ | KEY_WRITE)?;
        let current: String = environment.get_value("Path").unwrap_or_default();
        let bin = paths::bin_dir()?.display().to_string();
        if !current
            .split(';')
            .any(|entry| entry.eq_ignore_ascii_case(&bin))
        {
            environment.set_value("Path", &format!("{bin};{current}"))?;
            announce_environment_change();
            changed.push(PathBuf::from(r"HKCU\Environment\Path"));
        }
        let line = format!(
            r#"Invoke-Expression (& "{}" env --shell powershell | Out-String)"#,
            bvm.display()
        );
        for file in powershell_profiles() {
            if append_once(&file, &line)? {
                changed.push(file);
            }
        }
    }
    Ok(changed)
}

/// The PowerShell profiles to set up: the one the installer reports
/// (`$PROFILE`, which follows a OneDrive-redirected Documents folder), else
/// the default locations for PowerShell 7 and Windows PowerShell 5.1.
#[cfg(windows)]
fn powershell_profiles() -> Vec<PathBuf> {
    if let Some(profile) = std::env::var_os("BVM_POWERSHELL_PROFILE") {
        return vec![PathBuf::from(profile)];
    }
    std::env::var_os("USERPROFILE")
        .map(|home| {
            let documents = PathBuf::from(home).join("Documents");
            vec![
                documents.join(r"PowerShell\Microsoft.PowerShell_profile.ps1"),
                documents.join(r"WindowsPowerShell\Microsoft.PowerShell_profile.ps1"),
            ]
        })
        .unwrap_or_default()
}

/// Tells running programs (Explorer, and the terminals it starts) that the
/// user environment changed, so new windows see the new PATH without a sign
/// out.
#[cfg(windows)]
fn announce_environment_change() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE,
    };
    let area: Vec<u16> = "Environment\0".encode_utf16().collect();
    let mut result = 0usize;
    // SAFETY: a broadcast with a NUL-terminated string that outlives the call.
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            area.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            5000,
            &mut result,
        );
    }
}

/// Shell assignment for `bvm use`.
pub fn use_env(shell: &str, version: &str) -> String {
    match shell {
        "powershell" => format!("$env:BVM_BUN_VERSION = '{version}'"),
        "fish" => format!("set -gx BVM_BUN_VERSION '{version}'"),
        _ => format!("export BVM_BUN_VERSION='{version}'"),
    }
}

pub fn fish_env() -> Result<String> {
    let root = paths::root()?;
    Ok(format!(
        r#"set -gx BVM_DIR "{root}"
contains "$BVM_DIR/bin" $PATH; or set -gx PATH "$BVM_DIR/bin" $PATH
function bvm
  if test "$argv[1]" = use
    command bvm use --print-env --shell fish $argv[2] | source
  else
    command bvm $argv
  end
end
"#,
        root = root.display()
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::posix_startup_files;
    use std::fs;

    fn names(files: Vec<std::path::PathBuf>) -> Vec<String> {
        files
            .iter()
            .map(|file| file.file_name().unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn sets_up_the_users_own_shell_even_without_a_startup_file() {
        let home = std::env::temp_dir().join(format!("bvm-setup-{}", std::process::id()));
        fs::create_dir_all(&home).unwrap();
        // A fresh macOS account: zsh, no .zshrc yet.
        assert_eq!(names(posix_startup_files(&home, "zsh")), vec![".zshrc"]);
        let bash = names(posix_startup_files(&home, "bash"));
        assert!(bash.contains(&".bashrc".to_string()));
        assert_eq!(
            bash.contains(&".bash_profile".to_string()),
            cfg!(target_os = "macos")
        );
        // An existing startup file of another shell is kept up to date too.
        fs::write(home.join(".zshrc"), "").unwrap();
        assert!(names(posix_startup_files(&home, "bash")).contains(&".zshrc".to_string()));
        fs::remove_dir_all(&home).unwrap();
    }
}
