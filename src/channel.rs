//! Where versions come from. `1.4.2` is Bun's official release;
//! `1.4.2-absolute.1` is AbsoluteJS's patched build of it.

use anyhow::{Context, Result, anyhow, bail};
use std::fmt;
use std::io::Read;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Official,
    Absolute,
}

impl Channel {
    pub fn repository(self) -> &'static str {
        match self {
            Channel::Official => "oven-sh/bun",
            Channel::Absolute => "absolutejs/patched-bun",
        }
    }
}

/// An exact, installable version.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    pub bun: semver::Version,
    /// `Some(n)` for `<bun>+absolute.<n>` or a legacy hyphen label.
    pub absolute: Option<u32>,
    /// Preserve old release tags and installation directories verbatim.
    legacy_absolute: bool,
}

impl Version {
    pub fn parse(text: &str) -> Result<Self> {
        let text = text
            .trim()
            .trim_start_matches("bun-v")
            .trim_start_matches('v');
        let legacy_absolute = text.contains("-absolute.");
        let (base, absolute) = match text
            .split_once("+absolute.")
            .or_else(|| text.split_once("-absolute."))
        {
            Some((_, n)) if n.is_empty() || !n.bytes().all(|b| b.is_ascii_digit()) => {
                bail!("bad AbsoluteJS build number in {text}");
            }
            Some((base, n)) => (
                base,
                Some(
                    n.parse()
                        .with_context(|| format!("bad AbsoluteJS build number in {text}"))?,
                ),
            ),
            None => (text, None),
        };
        let bun =
            semver::Version::parse(base).with_context(|| format!("{text} is not a Bun version"))?;
        if !bun.build.is_empty() {
            bail!("{text}: unsupported build metadata");
        }
        if !bun.pre.is_empty() {
            bail!("{text}: prerelease and canary builds are not managed by bvm");
        }
        Ok(Self {
            bun,
            absolute,
            legacy_absolute,
        })
    }

    pub fn channel(&self) -> Channel {
        if self.absolute.is_some() {
            Channel::Absolute
        } else {
            Channel::Official
        }
    }

    pub fn tag(&self) -> String {
        format!("bun-v{self}")
    }

    pub fn download_url(&self, file: &str) -> String {
        format!(
            "https://github.com/{}/releases/download/{}/{file}",
            self.channel().repository(),
            self.tag().replace('+', "%2B")
        )
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.absolute {
            Some(n) => write!(
                f,
                "{}{}absolute.{n}",
                self.bun,
                if self.legacy_absolute { "-" } else { "+" }
            ),
            None => write!(f, "{}", self.bun),
        }
    }
}

pub fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .user_agent(concat!("bvm/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

pub fn get_bytes(url: &str) -> Result<Vec<u8>> {
    let response = agent()
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let mut body = Vec::new();
    response
        .into_body()
        .into_with_config()
        .limit(1 << 30)
        .reader()
        .read_to_end(&mut body)
        .with_context(|| format!("reading {url}"))?;
    Ok(body)
}

pub fn get_text(url: &str) -> Result<String> {
    String::from_utf8(get_bytes(url)?).map_err(|_| anyhow!("{url} is not UTF-8"))
}

/// Every published, non-prerelease version on a channel (GitHub API; set
/// `GITHUB_TOKEN` to lift the unauthenticated rate limit).
pub fn remote_versions(channel: Channel) -> Result<Vec<Version>> {
    let mut versions = Vec::new();
    for page in 1..=10 {
        let url = format!(
            "https://api.github.com/repos/{}/releases?per_page=100&page={page}",
            channel.repository()
        );
        let mut request = agent()
            .get(&url)
            .header("Accept", "application/vnd.github+json");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.header("Authorization", &format!("Bearer {token}"));
        }
        let body = request
            .call()
            .with_context(|| {
                format!(
                    "listing {} releases (set GITHUB_TOKEN if rate limited)",
                    channel.repository()
                )
            })?
            .into_body()
            .read_to_string()?;
        let releases: serde_json::Value = serde_json::from_str(&body)?;
        let releases = releases
            .as_array()
            .ok_or_else(|| anyhow!("unexpected GitHub response"))?;
        if releases.is_empty() {
            break;
        }
        for release in releases {
            if release["draft"].as_bool() == Some(true)
                || release["prerelease"].as_bool() == Some(true)
            {
                continue;
            }
            if let Some(tag) = release["tag_name"].as_str()
                && let Ok(version) = Version::parse(tag)
                && version.channel() == channel
            {
                versions.push(version);
            }
        }
    }
    versions.sort_by(|a, b| (&a.bun, a.absolute).cmp(&(&b.bun, b.absolute)));
    versions.dedup();
    Ok(versions)
}

/// The tag of a repository's latest release, from GitHub's `releases/latest`
/// redirect: no API call, so no rate limit.
pub fn latest_tag(repository: &str) -> Result<String> {
    let url = format!("https://github.com/{repository}/releases/latest");
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(concat!("bvm/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let response = agent
        .get(&url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .and_then(|location| location.split_once("/releases/tag/"))
        .map(|(_, tag)| tag.replace("%2B", "+").replace("%2b", "+"))
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| anyhow!("{repository} has no published release"))
}

/// The newest stable version on a channel.
fn latest_version(channel: Channel) -> Result<Version> {
    let tag = latest_tag(channel.repository())?;
    let version = Version::parse(&tag)
        .with_context(|| format!("{}'s latest release {tag}", channel.repository()))?;
    if version.channel() != channel {
        bail!(
            "{}'s latest release {tag} is not on that channel",
            channel.repository()
        );
    }
    Ok(version)
}

/// `latest`, `absolute`, or an exact version.
pub fn resolve_request(request: &str) -> Result<Version> {
    match request {
        "latest" => latest_version(Channel::Official),
        "absolute" | "absolute-latest" => latest_version(Channel::Absolute),
        exact => Version::parse(exact),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_official_and_absolute_versions() {
        let official = Version::parse("bun-v1.4.2").unwrap();
        assert_eq!(official.to_string(), "1.4.2");
        assert_eq!(official.channel(), Channel::Official);
        assert_eq!(
            official.download_url("SHASUMS256.txt.asc"),
            "https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/SHASUMS256.txt.asc"
        );
        let patched = Version::parse("1.4.2-absolute.1").unwrap();
        assert_eq!(patched.channel(), Channel::Absolute);
        assert_eq!(patched.tag(), "bun-v1.4.2-absolute.1");
        assert!(Version::parse("1.4.0-canary.1").is_err());
        assert!(Version::parse("nope").is_err());
        let metadata = Version::parse("bun-v1.4.3+absolute.2").unwrap();
        assert_eq!(metadata.channel(), Channel::Absolute);
        assert_eq!(metadata.to_string(), "1.4.3+absolute.2");
        assert_eq!(metadata.tag(), "bun-v1.4.3+absolute.2");
        assert!(
            metadata
                .download_url("bun.zip")
                .contains("bun-v1.4.3%2Babsolute.2/")
        );
        assert!(
            semver::VersionReq::parse(">=1.4.3")
                .unwrap()
                .matches(&metadata.bun)
        );
        assert_ne!(metadata, Version::parse("1.4.3-absolute.2").unwrap());
        for bad in [
            "1.4.3+other.1",
            "1.4.3+absolute.x",
            "1.4.3+absolute.+1",
            "1.4.3+absolute.1+other",
            "1.4.3-rc.1+absolute.1",
        ] {
            assert!(Version::parse(bad).is_err(), "{bad}");
        }
    }
}
