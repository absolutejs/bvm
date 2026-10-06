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

/// Adds `eval "$(bvm env)"` to the user's shell startup files (bash, zsh) and
/// the fish equivalent; on Windows, the PowerShell profile and the user PATH.
pub fn setup() -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    let bvm = paths::bin_dir()?.join(format!("bvm{}", crate::platform::exe_suffix()));
    #[cfg(unix)]
    {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let eval = format!(r#"eval "$("{}" env)""#, bvm.display());
        for rc in [".bashrc", ".zshrc"] {
            let file = home.join(rc);
            if (file.exists() || rc == ".bashrc") && append_once(&file, &eval)? {
                changed.push(file);
            }
        }
        let fish = home.join(".config/fish/conf.d/bvm.fish");
        if home.join(".config/fish").exists()
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
            changed.push(PathBuf::from(r"HKCU\Environment\Path"));
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            let file = PathBuf::from(profile)
                .join(r"Documents\PowerShell\Microsoft.PowerShell_profile.ps1");
            let line = format!(
                r#"Invoke-Expression (& "{}" env --shell powershell | Out-String)"#,
                bvm.display()
            );
            if append_once(&file, &line)? {
                changed.push(file);
            }
        }
    }
    Ok(changed)
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
