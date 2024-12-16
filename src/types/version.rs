use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Display;
use std::str::FromStr;

use chrono::{DateTime, FixedOffset};
use derive_more::derive::{Constructor, IsVariant};
use derive_more::Display as MoreDisplay;
use lazy_static::lazy_static;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{DeserializeFromStr, SerializeDisplay};

use crate::utils::macros::parse_variants;

/// Version format for release versions
/// in the form of `X.Y.Z`
#[derive(
    Clone, Debug, SerializeDisplay, DeserializeFromStr, PartialEq, Eq, PartialOrd, Ord, Constructor,
)]
pub(crate) struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Display for ReleaseVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReleaseVersion {
                major,
                minor,
                patch: 0,
            } => write!(f, "{major}.{minor}"),
            ReleaseVersion {
                major,
                minor,
                patch,
            } => write!(f, "{major}.{minor}.{patch}"),
        }
    }
}

impl FromStr for ReleaseVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?$").unwrap();
        }

        match RE.captures(s) {
            Some(caps) => Ok(Self::new(
                caps[1].parse().unwrap(),
                caps[2].parse().unwrap(),
                caps.get(3).map_or(0, |m| m.as_str().parse().unwrap()),
            )),
            None => Err(format!("Invalid version (expected X.Y[.Z], got: {s})")),
        }
    }
}

/// Version format for pre-release versions
/// in the form of `X.Y.Z-preN` or `X.Y.Z-rcN`
#[derive(
    Clone, Debug, SerializeDisplay, DeserializeFromStr, PartialEq, Eq, PartialOrd, Ord, Constructor,
)]
pub(crate) struct PreReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre: String, // /-(pre|rc)\d/
}

impl Display for PreReleaseVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PreReleaseVersion {
                major,
                minor,
                patch: 0,
                pre,
            } => write!(f, "{major}.{minor}-{pre}"),
            PreReleaseVersion {
                major,
                minor,
                patch,
                pre,
            } => write!(f, "{major}.{minor}.{patch}-{pre}"),
        }
    }
}

impl FromStr for PreReleaseVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        lazy_static! {
            static ref RE: Regex =
                Regex::new(r"^(\d+)\.(\d+)(?:\.(\d+))?-((?:pre|rc)\d+)$").unwrap();
        }

        match RE.captures(s) {
            Some(caps) => Ok(Self::new(
                caps[1].parse().unwrap(),
                caps[2].parse().unwrap(),
                caps.get(3).map_or(0, |m| m.as_str().parse().unwrap()),
                caps[4].to_string(),
            )),
            None => Err(format!(
                "Invalid version (expected X.Y[.Z]-<pre|rcN>, got: {s})"
            )),
        }
    }
}

/// Version format for snapshot versions
/// in the form of `XXwYYZ`, where `XX` is the year,
/// `YY` is the week, and `Z` is the iteration (a, b, c, ...)
#[derive(
    Clone, Debug, SerializeDisplay, DeserializeFromStr, PartialEq, Eq, PartialOrd, Ord, Constructor,
)]
pub(crate) struct SnapshotVersion {
    year: u8,          // 13-$currentyear
    week: u8,          // 01-52, probably
    iteration: String, // a, b, c ...
}

impl Display for SnapshotVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:02}w{:02}{}", self.year, self.week, self.iteration)
    }
}

impl FromStr for SnapshotVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        lazy_static! {
            static ref RE: Regex = Regex::new(r"^(\d{2})w(\d{2})([a-z])$").unwrap();
        }

        match RE.captures(s) {
            Some(caps) => Ok(Self::new(
                caps[1].parse().unwrap(),
                caps[2].parse().unwrap(),
                caps[3].to_string(),
            )),
            None => Err(format!("Invalid version (expected XXwYYZ, got: {s})")),
        }
    }
}

/// A version number, which can be one of the following:
/// - `Release`
/// - `PreRelease`
/// - `Snapshot`
/// - `Other`
#[derive(
    Clone, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, MoreDisplay, IsVariant,
)]
#[serde(untagged)]
pub(crate) enum VersionNumber {
    Release(ReleaseVersion),
    PreRelease(PreReleaseVersion),
    Snapshot(SnapshotVersion),
    Other(String), // fallback
}

// implements FromStr for VersionNumber
parse_variants!(VersionNumber {
    Release as ReleaseVersion,
    PreRelease as PreReleaseVersion,
    Snapshot as SnapshotVersion,
    Other as String,
});

/// A version of the game
///
/// Consists of an ID, a release type, the meta URL, and a release
/// timestamp
#[derive(Debug, Serialize, Deserialize, Eq)]
pub(crate) struct GameVersion {
    pub id: VersionNumber,
    #[serde(rename = "type")]
    pub release_type: String, // release, snapshot, old_beta, old_alpha. TODO: enum?
    pub url: String,
    pub time: DateTime<FixedOffset>,
    #[serde(rename = "releaseTime")]
    pub release_time: DateTime<FixedOffset>,
}

impl PartialEq for GameVersion {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Ord for GameVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.release_time.cmp(&other.release_time)
    }
}

impl PartialOrd for GameVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The latest versions of the game, as returned by the Mojang API
///
/// Includes the latest release and snapshot versions
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LatestVersions {
    pub release: VersionNumber,
    pub snapshot: VersionNumber,
}

/// A list of game versions, as returned by the Mojang API
///
/// Includes the latest versions, and a list of all versions
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct GameVersionList {
    pub latest: LatestVersions,
    pub versions: Vec<GameVersion>,
}

impl Iterator for GameVersionList {
    type Item = GameVersion;

    fn next(&mut self) -> Option<Self::Item> {
        self.versions.pop()
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct VersionDownload {
    sha1: String,
    size: u64,
    pub url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct JavaVersionInfo {
    component: String,
    #[serde(rename = "majorVersion")]
    pub major_version: u8,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct VersionMetadata {
    pub downloads: HashMap<String, VersionDownload>, // client, server, windows_server (legacy) + mappings
    pub id: VersionNumber,
    #[serde(rename = "javaVersion")]
    pub java_version: JavaVersionInfo,
    // the rest of the fields are not used

    // time: DateTime<FixedOffset>,
    // #[serde(rename = "releaseTime")]
    // releaseTime: DateTime<FixedOffset>,
    // #[serde(rename = "type")]
    // release_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_version_to_string() {
        let v = ReleaseVersion {
            major: 1,
            minor: 16,
            patch: 4,
        };
        assert_eq!(v.to_string(), "1.16.4");
    }

    #[test]
    fn release_version_deserialize() {
        let v: ReleaseVersion = serde_json::from_str(r#""1.16.4""#).unwrap();
        assert_eq!(
            v,
            ReleaseVersion {
                major: 1,
                minor: 16,
                patch: 4,
            }
        );
    }

    #[test]
    fn release_version_deserialize_invalid() {
        let v: Result<ReleaseVersion, _> = serde_json::from_str(r#""1.16.4-pre1""#);
        assert!(v.is_err());
    }

    #[test]
    fn release_version_compare() {
        let v1 = ReleaseVersion {
            major: 1,
            minor: 16,
            patch: 4,
        };
        let v2 = ReleaseVersion {
            major: 1,
            minor: 19,
            patch: 2,
        };
        assert!(v1 < v2);
    }

    #[test]
    fn release_version_equal() {
        let v1 = ReleaseVersion {
            major: 1,
            minor: 16,
            patch: 4,
        };
        let v2 = ReleaseVersion {
            major: 1,
            minor: 16,
            patch: 4,
        };
        assert!(v1 == v2);
    }

    #[test]
    fn prerelease_version_to_string() {
        let v = PreReleaseVersion {
            major: 1,
            minor: 16,
            patch: 4,
            pre: "pre1".to_string(),
        };
        assert_eq!(v.to_string(), "1.16.4-pre1");
    }

    #[test]
    fn prerelease_version_deserialize() {
        let v: PreReleaseVersion = serde_json::from_str(r#""1.16.4-pre1""#).unwrap();
        assert_eq!(
            v,
            PreReleaseVersion {
                major: 1,
                minor: 16,
                patch: 4,
                pre: "pre1".to_string(),
            }
        );
    }

    #[test]
    fn deserialze_version_number_enum() {
        let v: VersionNumber = serde_json::from_str(r#""1.16.4""#).unwrap();
        assert_eq!(
            v,
            VersionNumber::Release(ReleaseVersion {
                major: 1,
                minor: 16,
                patch: 4,
            })
        );

        let v: VersionNumber = serde_json::from_str(r#""1.16.4-pre1""#).unwrap();
        assert_eq!(
            v,
            VersionNumber::PreRelease(PreReleaseVersion {
                major: 1,
                minor: 16,
                patch: 4,
                pre: "pre1".to_string(),
            })
        );

        let v: VersionNumber = serde_json::from_str(r#""1.16.4-rc1""#).unwrap();
        assert_eq!(
            v,
            VersionNumber::PreRelease(PreReleaseVersion {
                major: 1,
                minor: 16,
                patch: 4,
                pre: "rc1".to_string(),
            })
        );

        let v: VersionNumber = serde_json::from_str(r#""20w45a""#).unwrap();
        assert_eq!(
            v,
            VersionNumber::Snapshot(SnapshotVersion {
                year: 20,
                week: 45,
                iteration: "a".to_string(),
            })
        );

        let v: VersionNumber = serde_json::from_str(r#""3D Shareware v1.34""#).unwrap();
        assert_eq!(v, VersionNumber::Other("3D Shareware v1.34".to_string()));
    }
}
