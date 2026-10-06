//! Nothing bvm installs runs unverified. Each release publishes
//! `SHASUMS256.txt`, a signature over it, and the zips it lists; a zip is
//! accepted only when its SHA-256 matches a line of a checksum list whose
//! signature verifies against a key compiled into bvm.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use ed25519_dalek::{Signature, VerifyingKey};
use pgp::composed::{CleartextSignedMessage, Deserializable, SignedPublicKey};
use pgp::types::KeyDetails;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Bun's release signing key (Robobun, Ed25519, fingerprint
/// F3DCC08A8572C0749B3E18888EAB4D40A7B22B59). Bun's own Docker images pin
/// the same fingerprint to verify `SHASUMS256.txt.asc`.
const OVEN_BUN_RELEASE_KEY: &str = include_str!("../keys/oven-bun-release.asc");
pub const OVEN_BUN_RELEASE_FINGERPRINT: &str = "f3dcc08a8572c0749b3e18888eab4d40a7b22b59";

/// AbsoluteJS release keys (Ed25519, raw public keys). A list, so a key can be
/// rotated while releases signed by the previous one stay installable.
const ABSOLUTEJS_RELEASE_KEYS: &[&str] =
    &[include_str!("../keys/absolutejs-release-ed25519.pub.hex")];

/// `name -> sha256` for every line of a checksum list.
pub type Checksums = HashMap<String, String>;

fn parse_checksums(text: &str) -> Checksums {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let hash = parts.next()?;
            let name = parts.next()?.trim_start_matches('*');
            (hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .then(|| (name.to_string(), hash.to_ascii_lowercase()))
        })
        .collect()
}

/// Bun's clearsigned `SHASUMS256.txt.asc`: verify the signature, then read the
/// checksums from the signed text only (never from anything around it).
pub fn official_checksums(clearsigned: &str) -> Result<Checksums> {
    let (key, _) = SignedPublicKey::from_string(OVEN_BUN_RELEASE_KEY)
        .context("bvm's embedded Bun release key is unreadable")?;
    let fingerprint = hex::encode(key.fingerprint().as_bytes());
    if fingerprint != OVEN_BUN_RELEASE_FINGERPRINT {
        bail!("bvm's embedded Bun release key is not the pinned key ({fingerprint})");
    }
    let (message, _) = CleartextSignedMessage::from_string(clearsigned)
        .context("SHASUMS256.txt.asc is not a clearsigned message")?;
    message
        .verify(&key)
        .map_err(|_| anyhow!("SHASUMS256.txt.asc is not signed by Bun's release key"))?;
    let checksums = parse_checksums(&message.signed_text());
    if checksums.is_empty() {
        bail!("the signed checksum list is empty");
    }
    Ok(checksums)
}

/// AbsoluteJS's `SHASUMS256.txt` and `SHASUMS256.txt.sig` (a base64 Ed25519
/// signature over the exact bytes of the checksum list).
pub fn absolute_checksums(list: &[u8], signature_file: &str) -> Result<Checksums> {
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_file.trim())
        .context("SHASUMS256.txt.sig is not base64")?;
    let signature = Signature::from_slice(&signature_bytes)
        .context("SHASUMS256.txt.sig is not an Ed25519 signature")?;
    let verified = ABSOLUTEJS_RELEASE_KEYS.iter().any(|encoded| {
        let Ok(bytes) = hex::decode(encoded.trim()) else {
            return false;
        };
        let Ok(raw) = <[u8; 32]>::try_from(bytes.as_slice()) else {
            return false;
        };
        let Ok(key) = VerifyingKey::from_bytes(&raw) else {
            return false;
        };
        key.verify_strict(list, &signature).is_ok()
    });
    if !verified {
        bail!("SHASUMS256.txt is not signed by an AbsoluteJS release key");
    }
    let text = std::str::from_utf8(list).context("SHASUMS256.txt is not UTF-8")?;
    let checksums = parse_checksums(text);
    if checksums.is_empty() {
        bail!("the signed checksum list is empty");
    }
    Ok(checksums)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// The download must be exactly the file the signed list names.
pub fn check_download(checksums: &Checksums, name: &str, bytes: &[u8]) -> Result<()> {
    let expected = checksums
        .get(name)
        .ok_or_else(|| anyhow!("{name} is not in the signed checksum list"))?;
    let actual = sha256_hex(bytes);
    if &actual != expected {
        bail!("{name} does not match its signed checksum (expected {expected}, got {actual})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_only_well_formed_checksum_lines() {
        let sums = parse_checksums(&format!(
            "{}  bun-linux-x64.zip\nnot a line\n",
            "a".repeat(64)
        ));
        assert_eq!(sums.len(), 1);
        assert_eq!(sums["bun-linux-x64.zip"], "a".repeat(64));
    }

    #[test]
    fn rejects_a_download_that_differs_from_the_signed_checksum() {
        let mut sums = Checksums::new();
        sums.insert("x.zip".into(), sha256_hex(b"right"));
        assert!(check_download(&sums, "x.zip", b"right").is_ok());
        assert!(check_download(&sums, "x.zip", b"wrong").is_err());
        assert!(check_download(&sums, "other.zip", b"right").is_err());
    }

    const BUN_1_4_2_SUMS: &str = include_str!("../tests/fixtures/bun-v1.4.2-SHASUMS256.txt.asc");

    #[test]
    fn verifies_bun_1_4_2_with_the_pinned_release_key() {
        let sums = official_checksums(BUN_1_4_2_SUMS).expect("Bun's real 1.4.2 checksums verify");
        assert_eq!(
            sums["bun-linux-x64.zip"].len(),
            64,
            "the signed list names the Linux x64 zip"
        );
        assert!(sums.len() >= 30);
    }

    #[test]
    fn rejects_bun_checksums_altered_after_signing() {
        // Flip one hex digit of one checksum: the signature no longer holds.
        let line = BUN_1_4_2_SUMS
            .lines()
            .find(|line| line.ends_with("bun-linux-x64.zip"))
            .expect("fixture has the linux zip");
        let first = line.chars().next().unwrap();
        let flipped = if first == '0' { '1' } else { '0' };
        let tampered = BUN_1_4_2_SUMS.replacen(line, &format!("{flipped}{}", &line[1..]), 1);
        assert!(official_checksums(&tampered).is_err());
    }

    #[test]
    fn rejects_an_unsigned_or_tampered_absolute_list() {
        let list = b"0000000000000000000000000000000000000000000000000000000000000000  bun-linux-x64.zip\n";
        let bogus = base64::engine::general_purpose::STANDARD.encode([7u8; 64]);
        assert!(absolute_checksums(list, &bogus).is_err());
    }
}
