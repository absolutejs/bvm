//! A Bun installed before bvm (Bun's own installer, Homebrew, npm, Scoop).
//! `bvm setup` finds it and installs the same version through bvm, verified,
//! as the default: once bvm's shims come first on PATH, `bun` still means the
//! version it meant before.

use crate::channel::Version;
use crate::paths;
use crate::platform::exe_suffix;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Existing {
    pub path: PathBuf,
    pub version: Version,
}

/// Where a Bun may already be: `$BUN_INSTALL/bin`, `~/.bun/bin`, then PATH.
fn candidates() -> Vec<PathBuf> {
    let name = format!("bun{}", exe_suffix());
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BUN_INSTALL").filter(|dir| !dir.is_empty()) {
        dirs.push(PathBuf::from(dir).join("bin"));
    }
    if let Some(home) = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }) {
        dirs.push(PathBuf::from(home).join(".bun").join("bin"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let file = dir.join(&name);
        if !files.contains(&file) {
            files.push(file);
        }
    }
    files
}

/// True when `file` is one of bvm's own shims, which would only report the
/// version bvm selects.
fn is_shim(file: &Path, bin: &Path) -> bool {
    match (file.canonicalize(), bin.canonicalize()) {
        (Ok(file), Ok(bin)) => file.starts_with(bin),
        _ => file.starts_with(bin),
    }
}

/// The first Bun outside bvm that runs and reports a version bvm manages.
pub fn find() -> Option<Existing> {
    let bin = paths::bin_dir().ok()?;
    candidates()
        .into_iter()
        .filter(|file| file.is_file() && !is_shim(file, &bin))
        .find_map(|path| {
            let output = Command::new(&path)
                .arg("--version")
                .env_remove("BVM_BUN_VERSION")
                .output()
                .ok()
                .filter(|output| output.status.success())?;
            let version = Version::parse(String::from_utf8_lossy(&output.stdout).trim()).ok()?;
            Some(Existing { path, version })
        })
}
