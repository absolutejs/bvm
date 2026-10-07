//! `bvm self uninstall`: remove bvm and leave one Bun behind, where Bun's own
//! installer puts it (`~/.bun/bin`, or `$BUN_INSTALL/bin`), so `bun`,
//! `bun upgrade` and Bun's docs work as if bvm had never been there.
//!
//! Everything is planned first and shown before anything changes. On a
//! terminal the user picks the Bun to keep with the arrow keys (the default
//! is preselected); elsewhere the flags decide, and `--yes` is required.

use crate::adopt::{self, Existing};
use crate::channel::Version;
use crate::platform::exe_suffix;
use crate::{install, paths, resolve, setup, ui};
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub struct Options {
    pub keep: Option<String>,
    pub keep_default: bool,
    pub remove_bun: bool,
    pub yes: bool,
}

/// What stays installed.
enum Keep {
    /// One of bvm's versions, copied to the standalone location.
    Version(Version),
    /// A Bun that was there before bvm; left exactly as it is.
    Existing(Existing),
    Nothing,
}

/// Where Bun's own installer puts Bun: `$BUN_INSTALL`, else `~/.bun`.
fn bun_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("BUN_INSTALL").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| anyhow!("cannot find a home directory"))?;
    Ok(PathBuf::from(home).join(".bun"))
}

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

/// The choice when nobody is asked: the default version (what `bun` runs
/// wherever no project picks one), as the Bun that was already there if it
/// is that same version; else that earlier Bun; else the newest installed.
fn preselected(
    installed: &[Version],
    default: Option<&Version>,
    existing: Option<&Existing>,
) -> usize {
    let existing_index = installed.len();
    if let Some(default) = default {
        if existing.is_some_and(|e| &e.version == default) {
            return existing_index;
        }
        if let Some(index) = installed.iter().position(|v| v == default) {
            return index;
        }
    }
    if existing.is_some() {
        return existing_index;
    }
    installed.len().saturating_sub(1)
}

fn choose(
    options: &Options,
    installed: Vec<Version>,
    default: Option<&Version>,
    existing: Option<Existing>,
) -> Result<Keep> {
    if options.remove_bun {
        return Ok(Keep::Nothing);
    }
    if let Some(text) = &options.keep {
        let wanted = Version::parse(text)?;
        if let Some(existing) = existing.filter(|e| e.version == wanted) {
            return Ok(Keep::Existing(existing));
        }
        if !installed.contains(&wanted) {
            bail!("Bun {wanted} is not installed; `bvm ls` lists the versions you can keep");
        }
        return Ok(Keep::Version(wanted));
    }
    if installed.is_empty() && existing.is_none() {
        return Ok(Keep::Nothing);
    }
    let index = preselected(&installed, default, existing.as_ref());
    let pick = if options.keep_default || !interactive() {
        index
    } else {
        let paint = ui::err();
        let mut items: Vec<String> = installed
            .iter()
            .map(|version| {
                let mut notes = Vec::new();
                if default == Some(version) {
                    notes.push("default".to_string());
                }
                if version.absolute.is_some() {
                    notes.push("AbsoluteJS build".to_string());
                }
                format!("{version}  {}", paint.dim(notes.join(", ")))
            })
            .collect();
        if let Some(existing) = &existing {
            let is_default = default == Some(&existing.version);
            items.push(format!(
                "{}  {}",
                existing.version,
                paint.dim(format!(
                    "{}already at {}, installed before bvm",
                    if is_default { "default, " } else { "" },
                    ui::tilde(&existing.path)
                ))
            ));
        }
        items.push("None: remove Bun too".into());
        ui::say("");
        dialoguer::Select::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Remove bvm. Which Bun should stay installed?")
            .items(&items)
            .default(index)
            .interact_opt()?
            .ok_or_else(|| anyhow!("cancelled; nothing was changed"))?
    };
    let existing_index = installed.len();
    Ok(if pick < existing_index {
        Keep::Version(installed[pick].clone())
    } else if pick == existing_index
        && let Some(existing) = existing
    {
        Keep::Existing(existing)
    } else {
        Keep::Nothing
    })
}

/// The startup files that need Bun's own PATH lines, as Bun's installer
/// writes them, because they do not already put Bun on PATH.
#[cfg(unix)]
fn files_missing_bun_path(bun_bin: &Path) -> Vec<(PathBuf, String)> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let shell = setup::shell_name();
    // BUN_INSTALL is Bun's home (`~/.bun`), the directory above `bin`.
    let shown = bun_bin
        .parent()
        .and_then(Path::parent)
        .map(|dir| dir.display().to_string())
        .unwrap_or_default()
        .replacen(&home.display().to_string(), "$HOME", 1);
    let mentions_bun = |file: &Path| {
        fs::read_to_string(file)
            .map(|text| {
                text.contains("BUN_INSTALL")
                    || text.contains(".bun/bin")
                    || text.contains(&bun_bin.display().to_string())
            })
            .unwrap_or(false)
    };
    let mut files = Vec::new();
    for file in setup::posix_startup_files(&home, &shell) {
        if !mentions_bun(&file) {
            files.push((
                file,
                format!("# bun\nexport BUN_INSTALL=\"{shown}\"\nexport PATH=\"$BUN_INSTALL/bin:$PATH\"\n"),
            ));
        }
    }
    let fish = home.join(".config/fish/config.fish");
    if (shell == "fish" || home.join(".config/fish").exists()) && !mentions_bun(&fish) {
        files.push((
            fish,
            format!("# bun\nset --export BUN_INSTALL \"{shown}\"\nset --export PATH $BUN_INSTALL/bin $PATH\n"),
        ));
    }
    files
}

#[cfg(unix)]
fn append(file: &Path, text: &str) -> Result<()> {
    let existing = fs::read_to_string(file).unwrap_or_default();
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)?;
    }
    let separator = if existing.is_empty() || existing.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::write(file, format!("{existing}{separator}\n{text}"))?;
    Ok(())
}

/// Links to bvm the installer made in a PATH directory (`~/.local/bin/bvm`).
#[cfg(unix)]
fn bvm_links(root: &Path) -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    [".local/bin/bvm", "bin/bvm"]
        .into_iter()
        .map(|name| home.join(name))
        .filter(|link| {
            fs::read_link(link)
                .map(|target| target.starts_with(root))
                .unwrap_or(false)
        })
        .collect()
}

/// Puts `source` at `target` (and `bunx` beside it) as Bun's installer would:
/// staged beside it and renamed into place, so it is never half-written.
fn place_bun(source: &Path, target: &Path) -> Result<()> {
    let dir = target.parent().context("no directory for Bun")?;
    fs::create_dir_all(dir)?;
    let staged = dir.join(format!(".bun{}.new", exe_suffix()));
    let _ = fs::remove_file(&staged);
    fs::copy(source, &staged).with_context(|| format!("copying Bun to {}", dir.display()))?;
    if cfg!(windows) && target.exists() {
        let _ = fs::remove_file(target);
    }
    fs::rename(&staged, target).with_context(|| format!("placing {}", target.display()))?;
    let bunx = dir.join(format!("bunx{}", exe_suffix()));
    let _ = fs::remove_file(&bunx);
    #[cfg(unix)]
    std::os::unix::fs::symlink(format!("bun{}", exe_suffix()), &bunx)?;
    #[cfg(windows)]
    if fs::hard_link(target, &bunx).is_err() {
        fs::copy(target, &bunx)?;
    }
    Ok(())
}

/// Deletes bvm's directory. Windows will not delete a running program, so
/// what cannot go now (bvm itself, and its other names) is moved to the temp
/// directory and a detached `cmd` removes it once bvm has exited.
fn remove_root(root: &Path) -> Result<()> {
    if fs::remove_dir_all(root).is_ok() || !root.exists() {
        return Ok(());
    }
    #[cfg(windows)]
    {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let aside = std::env::temp_dir().join(format!("bvm-uninstall-{stamp}"));
        fs::create_dir_all(&aside)?;
        if let Ok(entries) = fs::read_dir(paths::bin_dir()?) {
            for (index, entry) in entries.flatten().enumerate() {
                let _ = fs::rename(
                    entry.path(),
                    aside.join(format!("{index}-{}", entry.file_name().to_string_lossy())),
                );
            }
        }
        fs::remove_dir_all(root)
            .with_context(|| format!("removing {}; delete it by hand", root.display()))?;
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new(
            std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into()),
        )
        .raw_arg(format!(
            "/C ping -n 3 127.0.0.1 >NUL & rmdir /S /Q \"{}\"",
            aside.display()
        ))
        .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
        .spawn();
        return Ok(());
    }
    #[cfg(not(windows))]
    fs::remove_dir_all(root).with_context(|| format!("removing {}", root.display()))
}

/// Another `bun` that comes before `dir` on this PATH, outside bvm.
fn shadowing_bun(dir: &Path, root: &Path) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        if entry == dir {
            return None;
        }
        let candidate = entry.join(format!("bun{}", exe_suffix()));
        if candidate.is_file() && !candidate.starts_with(root) {
            return Some(candidate);
        }
    }
    None
}

pub fn run(options: Options) -> Result<()> {
    let root = paths::root()?;
    let paint = ui::err();
    let installed = install::installed()?;
    let default = resolve::default_version().ok().flatten();
    let existing = adopt::find();
    let bun_bin = bun_home()?.join("bin").join(format!("bun{}", exe_suffix()));
    let me = std::env::current_exe().ok();

    // The Bun that was there before bvm, when it is the same release as one of
    // bvm's, is offered once: keeping it changes nothing on disk.
    let choices: Vec<Version> = installed
        .iter()
        .filter(|version| existing.as_ref().is_none_or(|e| &e.version != *version))
        .cloned()
        .collect();
    let keep = choose(&options, choices, default.as_ref(), existing)?;

    // The plan, shown before anything changes.
    let mut plan: Vec<String> = Vec::new();
    let keeps_standalone = match &keep {
        Keep::Version(version) => {
            let replacing = bun_bin
                .is_file()
                .then(|| format!(" {}", paint.dim("(replacing the Bun there now)")));
            plan.push(format!(
                "Keep Bun {} at {}{}",
                paint.version(version),
                paint.path(&bun_bin),
                replacing.unwrap_or_default()
            ));
            if version.absolute.is_some() {
                plan.push(format!(
                    "Note: this is an AbsoluteJS build; {} would replace it with official Bun",
                    paint.cmd("bun upgrade")
                ));
            }
            true
        }
        Keep::Existing(existing) => {
            plan.push(format!(
                "Keep Bun {} at {} as it is",
                paint.version(&existing.version),
                paint.path(&existing.path)
            ));
            existing.path == bun_bin
        }
        Keep::Nothing => {
            if bun_bin.is_file() {
                plan.push(format!("Remove the Bun at {}", paint.path(&bun_bin)));
            }
            false
        }
    };
    #[cfg(unix)]
    let bun_path_files = if keeps_standalone {
        files_missing_bun_path(&bun_bin)
    } else {
        Vec::new()
    };
    #[cfg(unix)]
    for (file, _) in &bun_path_files {
        plan.push(format!(
            "Add {} to PATH in {}",
            paint.path(bun_bin.parent().unwrap_or(&bun_bin)),
            paint.path(file)
        ));
    }
    #[cfg(windows)]
    let bun_dir = bun_bin.parent().map(Path::to_path_buf).unwrap_or_default();
    #[cfg(windows)]
    let add_bun_to_path = keeps_standalone && !setup::on_user_path(&bun_dir)?;
    #[cfg(windows)]
    if add_bun_to_path {
        plan.push(format!("Add {} to your user PATH", paint.path(&bun_dir)));
    }
    let blocks = setup::files_with_block();
    for file in &blocks {
        plan.push(format!("Remove bvm's lines from {}", paint.path(file)));
    }
    #[cfg(windows)]
    if setup::on_user_path(&paths::bin_dir()?)? {
        plan.push(format!(
            "Take {} off your user PATH",
            paint.path(&paths::bin_dir()?)
        ));
    }
    #[cfg(unix)]
    let links = bvm_links(&root);
    #[cfg(unix)]
    for link in &links {
        plan.push(format!("Remove the link {}", paint.path(link)));
    }
    let count = installed.len();
    plan.push(format!(
        "Delete {}{}",
        paint.path(&root),
        if count > 0 {
            format!(
                " {}",
                paint.dim(format!(
                    "({count} Bun version{})",
                    if count == 1 { "" } else { "s" }
                ))
            )
        } else {
            String::new()
        }
    ));

    ui::say("");
    ui::say(format!("  {}", paint.bold("bvm will:")));
    for step in &plan {
        ui::note(step);
    }
    ui::say("");
    ui::say(format!(
        "  {}",
        paint.dim(".bun-version files and engines.bun stop choosing Bun versions.")
    ));
    ui::say("");
    if !options.yes {
        if !interactive() {
            bail!(
                "not a terminal, so nothing was asked and nothing changed; rerun with `--yes` (and `--keep <version>`, `--keep-default` or `--remove-bun`)"
            );
        }
        let go = dialoguer::Confirm::with_theme(&dialoguer::theme::ColorfulTheme::default())
            .with_prompt("Continue?")
            .default(true)
            .interact_opt()?
            .unwrap_or(false);
        if !go {
            ui::note("Nothing was changed");
            return Ok(());
        }
    }

    // Bun first, so a failure below never leaves the user without one.
    match &keep {
        Keep::Version(version) => {
            place_bun(&paths::bun_binary(version)?, &bun_bin)?;
            ui::done(format!(
                "Bun {} is at {}",
                paint.version(version),
                paint.path(&bun_bin)
            ));
        }
        Keep::Existing(existing) => {
            // Bun's installer always puts `bunx` beside `bun`; bvm's own
            // `bunx` is going away, so make sure that one is there.
            let bunx = existing
                .path
                .with_file_name(format!("bunx{}", exe_suffix()));
            if existing.path == bun_bin && fs::symlink_metadata(&bunx).is_err() {
                #[cfg(unix)]
                std::os::unix::fs::symlink(format!("bun{}", exe_suffix()), &bunx)?;
                #[cfg(windows)]
                if fs::hard_link(&existing.path, &bunx).is_err() {
                    fs::copy(&existing.path, &bunx)?;
                }
            }
        }
        Keep::Nothing => {
            if bun_bin.is_file() {
                fs::remove_file(&bun_bin)?;
                let bunx = bun_bin.with_file_name(format!("bunx{}", exe_suffix()));
                let _ = fs::remove_file(bunx);
                // Only when empty: ~/.bun also holds global packages and the
                // install cache, which are not bvm's to delete.
                if let Some(bin) = bun_bin.parent() {
                    let _ = fs::remove_dir(bin);
                    if let Some(home) = bin.parent() {
                        let _ = fs::remove_dir(home);
                    }
                }
                ui::done(format!("Removed {}", paint.path(&bun_bin)));
            }
        }
    }
    #[cfg(unix)]
    for (file, text) in &bun_path_files {
        append(file, text)?;
        ui::done(format!("Added Bun to PATH in {}", paint.path(file)));
    }
    #[cfg(windows)]
    if add_bun_to_path && setup::add_to_user_path(&bun_dir)? {
        ui::done(format!("Added {} to your user PATH", paint.path(&bun_dir)));
    }
    for file in &blocks {
        setup::remove_block(file)?;
        ui::done(format!("Removed bvm's lines from {}", paint.path(file)));
    }
    #[cfg(windows)]
    if setup::remove_from_user_path(&paths::bin_dir()?)? {
        ui::done("Took bvm off your user PATH");
    }
    #[cfg(unix)]
    for link in &links {
        fs::remove_file(link)?;
        ui::done(format!("Removed {}", paint.path(link)));
    }
    remove_root(&root)?;
    ui::done(format!("Deleted {}", paint.path(&root)));

    ui::say("");
    ui::say(format!("  {}", paint.green("bvm is uninstalled.")));
    if let Keep::Existing(existing) = &keep
        && existing.path != bun_bin
    {
        ui::note(format!(
            "Your Bun stays at {}, managed by whatever installed it",
            paint.path(&existing.path)
        ));
    }
    if let Keep::Nothing = &keep
        && let Some(other) = adopt::find()
    {
        ui::note(format!(
            "Another Bun is still at {}; remove it the way it was installed",
            paint.path(&other.path)
        ));
    }
    if keeps_standalone
        && let Some(dir) = bun_bin.parent()
        && let Some(other) = shadowing_bun(dir, &root)
    {
        ui::warn(format!(
            "{} comes before {} on PATH, so `bun` runs that one",
            paint.path(&other),
            paint.path(dir)
        ));
    }
    if let Some(me) = &me
        && !me.starts_with(&root)
    {
        ui::note(format!(
            "This bvm ({}) came from elsewhere; remove it the way you installed it (npm: `npm uninstall --global @absolutejs/bvm`)",
            paint.path(me)
        ));
    }
    if cfg!(windows) {
        ui::note("Open a new terminal to pick up the change");
    } else if keeps_standalone {
        let dir = bun_bin.parent().unwrap_or(&bun_bin);
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = dir.display().to_string().replacen(&home, "$HOME", 1);
        ui::note(format!(
            "Open a new terminal, or run `export PATH=\"{dir}:$PATH\"` to use Bun in this one"
        ));
    } else {
        ui::note("Open a new terminal to pick up the change");
    }
    ui::say("");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn preselects_the_default_then_an_earlier_bun_then_the_newest() {
        let installed = vec![v("1.3.9"), v("1.4.2"), v("1.4.2-absolute.1")];
        assert_eq!(preselected(&installed, Some(&v("1.3.9")), None), 0);
        let earlier = Existing {
            path: PathBuf::from("/home/u/.bun/bin/bun"),
            version: v("1.4.2"),
        };
        // The default is the same version as the Bun that was already
        // there: keep that one, nothing to copy.
        assert_eq!(
            preselected(&installed, Some(&v("1.4.2")), Some(&earlier)),
            3
        );
        assert_eq!(
            preselected(&installed, Some(&v("1.3.9")), Some(&earlier)),
            0
        );
        assert_eq!(preselected(&installed, None, Some(&earlier)), 3);
        assert_eq!(preselected(&installed, None, None), 2);
    }
}
