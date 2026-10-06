//! Which Bun a `bun` call runs: `BVM_BUN_VERSION` (set by `bvm use`), then the
//! nearest `.bun-version`, then the nearest `package.json` `engines.bun`, then
//! the default.

use crate::channel::Version;
use crate::{install, paths};
use anyhow::{Context, Result, anyhow, bail};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Resolved {
    pub version: Version,
    pub source: String,
}

fn nearest(start: &Path, name: &str) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(name))
        .find(|file| file.is_file())
}

/// The highest installed version whose Bun version satisfies `range`. An
/// AbsoluteJS build counts as the Bun version it patches; on a tie the
/// default wins, then the official build.
fn best_installed(
    range: &semver::VersionReq,
    default: Option<&Version>,
) -> Result<Option<Version>> {
    let matching: Vec<Version> = install::installed()?
        .into_iter()
        .filter(|version| range.matches(&version.bun))
        .collect();
    if let Some(default) = default
        && matching.contains(default)
    {
        return Ok(Some(default.clone()));
    }
    Ok(matching
        .into_iter()
        .max_by(|a, b| (&a.bun, b.absolute.is_some()).cmp(&(&b.bun, a.absolute.is_some()))))
}

pub fn default_version() -> Result<Option<Version>> {
    match fs::read_to_string(paths::default_file()?) {
        Ok(text) => Ok(Some(Version::parse(&text)?)),
        Err(_) => Ok(None),
    }
}

pub fn resolve(cwd: &Path) -> Result<Resolved> {
    if let Ok(pinned) = std::env::var("BVM_BUN_VERSION")
        && !pinned.trim().is_empty()
    {
        return Ok(Resolved {
            version: Version::parse(&pinned)?,
            source: "BVM_BUN_VERSION (bvm use)".into(),
        });
    }
    if let Some(file) = nearest(cwd, ".bun-version") {
        let text = fs::read_to_string(&file)?;
        return Ok(Resolved {
            version: Version::parse(&text).with_context(|| format!("{}", file.display()))?,
            source: file.display().to_string(),
        });
    }
    let default = default_version()?;
    if let Some(file) = nearest(cwd, "package.json") {
        let manifest: serde_json::Value = serde_json::from_str(&fs::read_to_string(&file)?)
            .with_context(|| format!("{} is not valid JSON", file.display()))?;
        if let Some(range) = manifest["engines"]["bun"].as_str() {
            let req = semver::VersionReq::parse(range)
                .with_context(|| format!("engines.bun \"{range}\" in {}", file.display()))?;
            let version = best_installed(&req, default.as_ref())?.ok_or_else(|| {
                anyhow!(
                    "{} needs Bun {range} and no installed version satisfies it; run `bvm install latest`",
                    file.display()
                )
            })?;
            return Ok(Resolved {
                version,
                source: format!("engines.bun in {}", file.display()),
            });
        }
    }
    match default {
        Some(version) => Ok(Resolved {
            version,
            source: "default".into(),
        }),
        None => {
            bail!("no Bun version selected; run `bvm install latest` then `bvm default latest`")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_nearest_file_walking_up() {
        let root = std::env::temp_dir().join(format!("bvm-resolve-{}", std::process::id()));
        let deep = root.join("a/b/c");
        fs::create_dir_all(&deep).unwrap();
        fs::write(root.join("a/.bun-version"), "1.4.2").unwrap();
        assert_eq!(
            nearest(&deep, ".bun-version"),
            Some(root.join("a/.bun-version"))
        );
        assert_eq!(nearest(&deep, "nothing-here"), None);
        fs::remove_dir_all(&root).unwrap();
    }
}
