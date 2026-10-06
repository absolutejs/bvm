//! `~/.bvm` (`%USERPROFILE%\.bvm` on Windows), or `BVM_DIR`.

use crate::channel::Version;
use crate::platform::exe_suffix;
use anyhow::{Result, anyhow};
use std::path::PathBuf;

pub fn root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("BVM_DIR").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or_else(|| anyhow!("cannot find a home directory; set BVM_DIR"))?;
    Ok(PathBuf::from(home).join(".bvm"))
}

pub fn bin_dir() -> Result<PathBuf> {
    Ok(root()?.join("bin"))
}

pub fn versions_dir() -> Result<PathBuf> {
    Ok(root()?.join("versions"))
}

pub fn version_dir(version: &Version) -> Result<PathBuf> {
    Ok(versions_dir()?.join(version.to_string()))
}

pub fn bun_binary(version: &Version) -> Result<PathBuf> {
    Ok(version_dir(version)?.join(format!("bun{}", exe_suffix())))
}

pub fn default_file() -> Result<PathBuf> {
    Ok(root()?.join("default"))
}
