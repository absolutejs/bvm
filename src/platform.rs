//! Which release zip fits this machine. Names follow Bun's release assets
//! (`bun-<os>-<arch>[-musl][-baseline].zip`); AbsoluteJS's builds use the same
//! names, without `-baseline` (they are already built for the x64 baseline).

use crate::channel::Channel;
use anyhow::{Result, bail};

pub fn exe_suffix() -> &'static str {
    if cfg!(windows) { ".exe" } else { "" }
}

fn os() -> Result<&'static str> {
    Ok(match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        "windows" => "windows",
        "freebsd" => "freebsd",
        other => bail!("Bun does not publish builds for {other}"),
    })
}

fn arch() -> Result<&'static str> {
    Ok(match std::env::consts::ARCH {
        "x86_64" => "x64",
        "aarch64" => "aarch64",
        other => bail!("Bun does not publish builds for {other}"),
    })
}

/// Alpine and other musl systems need the musl build. Decided by the host's
/// dynamic loader, never by how bvm itself was built: bvm's Linux binaries are
/// static musl builds that also run on glibc systems.
fn is_musl() -> bool {
    std::fs::read_dir("/lib")
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().starts_with("ld-musl-"))
        })
        .unwrap_or(false)
}

/// Bun's default x64 build needs AVX2; older CPUs need `-baseline`.
fn needs_baseline() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        !std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        false
    }
}

/// The asset stem for this machine, e.g. `bun-linux-x64-musl`.
pub fn asset_stem(channel: Channel) -> Result<String> {
    let os = os()?;
    let arch = arch()?;
    let mut stem = format!("bun-{os}-{arch}");
    if os == "linux" && is_musl() {
        stem.push_str("-musl");
    }
    if channel == Channel::Official && arch == "x64" && needs_baseline() {
        stem.push_str("-baseline");
    }
    Ok(stem)
}
