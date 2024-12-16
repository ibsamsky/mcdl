use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use color_eyre::eyre::{eyre, Result};
use lazy_static::lazy_static;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{debug, instrument};

use crate::common::{PROJ_DIRS, REQWEST_CLIENT};
use crate::types::net::CachedResponse;
use crate::types::version::{GameVersion, GameVersionList, VersionMetadata};

lazy_static! {
    static ref CACHE_BASE_DIR: PathBuf = PROJ_DIRS.cache_dir().to_path_buf();
}

const PISTON_API_URL: &str = "https://piston-meta.mojang.com/";
const FABRIC_API_URL: &str = "https://meta.fabricmc.net/";

const CACHE_EXPIRATION_TIME: u64 = 60 * 10; // 10 minutes

#[inline]
fn api_path(path: &str) -> String {
    format!("{PISTON_API_URL}{path}")
}

#[inline]
fn fabric_api_path(path: &str) -> String {
    format!("{FABRIC_API_URL}{path}")
}

#[instrument(err)]
pub(crate) async fn get_version_manifest() -> Result<GameVersionList> {
    let cache_file = CACHE_BASE_DIR.join("manifest.mpk");

    get_maybe_cached(&api_path("mc/game/version_manifest.json"), &cache_file).await
}

#[instrument(err, skip(version), fields(version = %version.id))]
pub(crate) async fn get_version_metadata(version: &GameVersion) -> Result<VersionMetadata> {
    let cache_file = CACHE_BASE_DIR.join(format!("{}.mpk", version.id));

    get_maybe_cached(&version.url, &cache_file).await
}

#[instrument(err)] // ret is huge
pub(crate) async fn get_maybe_cached<T>(url: &str, cache_file: &PathBuf) -> Result<T>
where T: Serialize + for<'de> Deserialize<'de> {
    if let Ok(cached) = CachedResponse::<T>::from_file(&cache_file).await {
        if !cached.is_expired() {
            let mut msg = "Using cached response".to_string();
            if let Ok(elapsed) = cached.expires.duration_since(SystemTime::now()) {
                let (minutes, seconds) = (elapsed.as_secs() / 60, elapsed.as_secs() % 60);
                let milis = elapsed.subsec_millis();
                msg.push_str(&format!(
                    " expiring in {minutes:02}:{seconds:02}.{milis:03}"
                ));
            }
            debug!("{msg}");
            return Ok(cached.data);
        }
    }

    debug!("Downloading fresh data");
    let response: T = REQWEST_CLIENT.get(url).send().await?.json().await?;

    let cached_response = CachedResponse::new(
        &response,
        SystemTime::now() + Duration::from_secs(CACHE_EXPIRATION_TIME),
    );
    cached_response.save(&cache_file).await?;
    debug!("Saved cached response");

    Ok(response)
}

#[instrument(err)]
pub(crate) async fn download_jre(major_version: &u8) -> Result<Bytes> {
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{feature_version}/{release_type}/{os}/{arch}/{image_type}/{jvm_impl}/{heap_size}/{vendor}",
        feature_version = major_version,
        release_type = "ga",
        os = match std::env::consts::OS { "macos" => "mac", os => os },
        arch = std::env::consts::ARCH,
        image_type = "jre",
        jvm_impl = "hotspot",
        heap_size = "normal",
        vendor = "eclipse",
    );

    debug!(url, "Downloading JRE");
    let response = REQWEST_CLIENT.get(&url).send().await?;

    match response.status() {
        StatusCode::TEMPORARY_REDIRECT | StatusCode::OK => Ok(response.bytes().await?),
        StatusCode::BAD_REQUEST => Err(eyre!("Bad input parameter in URL: {url}")),
        StatusCode::NOT_FOUND => Err(eyre!("No binary found for the given parameters: {url}")),
        status => Err(eyre!("Unexpected error (status code {status}): {url}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_version_manifest() {
        let manifest = get_version_manifest().await.unwrap();
        assert!(!manifest.versions.is_empty());
    }

    #[tokio::test]
    async fn test_get_version_metadata() {
        let manifest = get_version_manifest().await.unwrap();
        let version = manifest.versions.get(0).unwrap();
        let metadata = get_version_metadata(version).await.unwrap();
        assert!(metadata.downloads.get("server").is_some());
    }

    #[tokio::test]
    async fn test_download_jre() {
        let version = match std::env::consts::OS {
            "macos" => 11, // Adoptium doesn't have JRE 8 for aarch64 macOS
            _ => 8,
        };

        let jre = download_jre(&version).await.unwrap();
        assert!(!jre.is_empty());
    }
}
