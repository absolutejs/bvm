//! Download, verify, then install. A version directory only ever appears
//! complete: the binary is extracted beside it and renamed into place.

use crate::channel::{Channel, Version, get_bytes, get_text};
use crate::platform::{asset_stem, exe_suffix};
use crate::verify::{absolute_checksums, check_download, official_checksums};
use crate::{paths, ui};
use anyhow::{Context, Result, anyhow};
use std::fs;
use std::io::Read;

pub fn is_installed(version: &Version) -> Result<bool> {
    Ok(paths::bun_binary(version)?.is_file())
}

pub fn installed() -> Result<Vec<Version>> {
    let dir = paths::versions_dir()?;
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut versions: Vec<Version> = entries
        .flatten()
        .filter_map(|entry| Version::parse(&entry.file_name().to_string_lossy()).ok())
        .filter(|version| {
            paths::bun_binary(version)
                .map(|bin| bin.is_file())
                .unwrap_or(false)
        })
        .collect();
    versions.sort_by(|a, b| (&a.bun, a.absolute).cmp(&(&b.bun, b.absolute)));
    Ok(versions)
}

pub fn install(version: &Version) -> Result<()> {
    if is_installed(version)? {
        let paint = ui::err();
        ui::done(format!(
            "Bun {} is already installed",
            paint.version(version)
        ));
        return Ok(());
    }
    let stem = asset_stem(version.channel())?;
    let zip_name = format!("{stem}.zip");
    let paint = ui::err();
    ui::working(format!(
        "Downloading Bun {} {}",
        paint.version(version),
        paint.dim(format!("({zip_name})"))
    ));
    let unpublished = |error: anyhow::Error| match error.downcast_ref::<ureq::Error>() {
        Some(ureq::Error::StatusCode(404)) => anyhow!(
            "Bun {version} is not a published release; `bvm ls-remote{}` lists the ones that are",
            if version.channel() == Channel::Absolute {
                " --absolute"
            } else {
                ""
            }
        ),
        _ => error,
    };
    let checksums = match version.channel() {
        Channel::Official => {
            let signed =
                get_text(&version.download_url("SHASUMS256.txt.asc")).map_err(unpublished)?;
            official_checksums(&tampered_for_tests(signed))?
        }
        Channel::Absolute => {
            let list = get_bytes(&version.download_url("SHASUMS256.txt")).map_err(unpublished)?;
            let signature = get_text(&version.download_url("SHASUMS256.txt.sig"))
                .context("this AbsoluteJS build has no signature (SHASUMS256.txt.sig); bvm will not install it")?;
            absolute_checksums(&list, &signature)?
        }
    };
    let archive = get_bytes(&version.download_url(&zip_name))?;
    check_download(&checksums, &zip_name, &archive)?;
    ui::done(match version.channel() {
        Channel::Official => "Verified Bun's PGP signature and the checksum",
        Channel::Absolute => "Verified the AbsoluteJS signature and the checksum",
    });

    let binary_name = format!("bun{}", exe_suffix());
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))
        .context("the release zip is unreadable")?;
    let entry_name = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().map(|entry| entry.name().to_string()))
        .find(|name| name.rsplit('/').next() == Some(binary_name.as_str()))
        .ok_or_else(|| anyhow!("{zip_name} has no {binary_name}"))?;
    let mut bytes = Vec::new();
    zip.by_name(&entry_name)?.read_to_end(&mut bytes)?;

    let target = paths::version_dir(version)?;
    let staging = target.with_extension("partial");
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;
    let staged_binary = staging.join(&binary_name);
    fs::write(&staged_binary, &bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&staged_binary, fs::Permissions::from_mode(0o755))?;
    }
    let _ = fs::remove_dir_all(&target);
    fs::rename(&staging, &target).with_context(|| format!("moving Bun {version} into place"))?;
    ui::done(format!("Installed Bun {}", paint.version(version)));
    Ok(())
}

/// Debug builds only: `BVM_TEST_TAMPER=1` changes one checksum digit so the
/// end-to-end tests can prove a list that no longer matches its signature is
/// refused. It can only make verification fail; release builds omit it.
fn tampered_for_tests(signed: String) -> String {
    if !cfg!(debug_assertions) || std::env::var("BVM_TEST_TAMPER").as_deref() != Ok("1") {
        return signed;
    }
    let Some(line) = signed
        .lines()
        .find(|line| line.len() > 64 && line.as_bytes()[0].is_ascii_hexdigit())
    else {
        return signed;
    };
    let flipped = if line.starts_with('0') { '1' } else { '0' };
    signed.replacen(line, &format!("{flipped}{}", &line[1..]), 1)
}

pub fn uninstall(version: &Version) -> Result<()> {
    let dir = paths::version_dir(version)?;
    if !dir.exists() {
        return Err(anyhow!("Bun {version} is not installed"));
    }
    fs::remove_dir_all(&dir)?;
    ui::done(format!("Removed Bun {}", ui::err().version(version)));
    Ok(())
}
