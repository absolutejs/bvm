//! bvm's own binary: placing it as `bvm`/`bun`/`bunx`, and replacing it with a
//! newer signed release (`bvm self update`).

use crate::channel::{get_bytes, get_text, latest_tag};
use crate::paths;
use crate::platform::exe_suffix;
use crate::verify::{absolute_checksums, check_download};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

const REPOSITORY: &str = "absolutejs/bvm";
const SHIM_NAMES: [&str; 3] = ["bvm", "bun", "bunx"];

/// The release asset for this machine: `bvm-<os>-<arch>[.exe]`. Linux builds
/// are static (musl), so one binary serves glibc and Alpine alike.
pub fn asset_name() -> Result<String> {
    let os = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        other => bail!("bvm is not built for {other}"),
    };
    let arch = match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        other => bail!("bvm is not built for {other}"),
    };
    Ok(format!("bvm-{os}-{arch}{}", exe_suffix()))
}

/// Executables Windows would not let us overwrite while running are renamed
/// aside first; remove the leftovers from earlier updates.
fn clear_leftovers(bin: &Path) {
    if let Ok(entries) = fs::read_dir(bin) {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().contains(".old-") {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}

/// Make `bvm`, `bun` and `bunx` in `~/.bvm/bin` all be `source`: hard links
/// where possible, copies otherwise. Each name is staged beside its target and
/// renamed over it, so a shim is never missing or half-written.
pub fn place_shims(source: &Path) -> Result<PathBuf> {
    let bin = paths::bin_dir()?;
    fs::create_dir_all(&bin)?;
    clear_leftovers(&bin);
    let suffix = exe_suffix();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    for name in SHIM_NAMES {
        let target = bin.join(format!("{name}{suffix}"));
        let staged = bin.join(format!(".{name}{suffix}.new"));
        let _ = fs::remove_file(&staged);
        if fs::hard_link(source, &staged).is_err() {
            fs::copy(source, &staged)
                .with_context(|| format!("copying bvm to {}", staged.display()))?;
        }
        if cfg!(windows) && target.exists() {
            // A running .exe cannot be replaced, but it can be renamed.
            let _ = fs::rename(&target, bin.join(format!("{name}{suffix}.old-{stamp}")));
        }
        fs::rename(&staged, &target).with_context(|| format!("placing {}", target.display()))?;
    }
    Ok(bin)
}

pub fn update() -> Result<()> {
    let tag = latest_tag(REPOSITORY)?;
    if !tag.starts_with('v') {
        bail!("bvm's latest release tag {tag} is not a version");
    }
    let latest = semver::Version::parse(tag.trim_start_matches('v'))?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;
    if latest <= current {
        eprintln!("bvm: {current} is the latest release");
        return Ok(());
    }
    let asset = asset_name()?;
    let base = format!("https://github.com/{REPOSITORY}/releases/download/{tag}");
    eprintln!("bvm: verifying bvm {latest} ({asset})");
    let list = get_bytes(&format!("{base}/SHASUMS256.txt"))?;
    let signature = get_text(&format!("{base}/SHASUMS256.txt.sig"))?;
    let checksums = absolute_checksums(&list, &signature)?;
    let binary = get_bytes(&format!("{base}/{asset}"))?;
    check_download(&checksums, &asset, &binary)?;
    eprintln!("bvm: signature and checksum verified");

    let bin = paths::bin_dir()?;
    fs::create_dir_all(&bin)?;
    let downloaded = bin.join(format!(".bvm-{latest}{}", exe_suffix()));
    fs::write(&downloaded, &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&downloaded, fs::Permissions::from_mode(0o755))?;
    }
    place_shims(&downloaded)?;
    let _ = fs::remove_file(&downloaded);
    eprintln!("bvm: updated to {latest} in {}", bin.display());
    let me = std::env::current_exe().ok();
    if let Some(me) = me
        && !me.starts_with(&bin)
    {
        eprintln!(
            "bvm: this bvm ({}) is outside {}; update it the way you installed it",
            me.display(),
            bin.display()
        );
    }
    Ok(())
}
