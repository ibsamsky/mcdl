use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use color_eyre::eyre::{Result, eyre};
use reqwest::header::{self, HeaderMap};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{debug, instrument};

use crate::cli::MCDL_VERSION;
use crate::paths::PROJ_DIRS;
use crate::types::version::{GameVersion, GameVersionList, VersionMetadata};

pub(crate) static REQWEST_CLIENT: LazyLock<Client> = LazyLock::new(|| {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_str(&format!(
            "mcdl/{} ({})",
            MCDL_VERSION.as_str(),
            env!("CARGO_PKG_HOMEPAGE")
        ))
        .expect("failed to build user agent header"),
    );

    Client::builder()
        .default_headers(headers)
        .tcp_keepalive(Some(Duration::from_secs(10)))
        .build()
        .expect("failed to build reqwest client")
});

static CACHE_BASE_DIR: LazyLock<PathBuf> = LazyLock::new(|| PROJ_DIRS.cache_dir().to_path_buf());

const PISTON_API_URL: &str = "https://piston-meta.mojang.com/";
// const FABRIC_API_URL: &str = "https://meta.fabricmc.net/";

const CACHE_EXPIRATION_TIME: u64 = 60 * 10; // 10 minutes

#[derive(Serialize, Deserialize)]
struct CachedResponse<T> {
    data: T,
    expires: SystemTime,
}

impl<T> CachedResponse<T> {
    fn new(data: T, expires: SystemTime) -> Self {
        Self { data, expires }
    }

    fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires
    }

    async fn from_file(path: impl AsRef<Path>) -> Result<Self>
    where Self: for<'de> Deserialize<'de> {
        let data = fs::read(path).await?;
        Ok(rmp_serde::from_slice(&data)?)
    }

    async fn save(&self, path: impl AsRef<Path>) -> Result<()>
    where Self: Serialize {
        let data = rmp_serde::to_vec(self)?;
        fs::create_dir_all(path.as_ref().parent().expect("infallible")).await?;
        fs::write(path, data).await?;
        Ok(())
    }
}

#[inline]
fn api_path(path: &str) -> String {
    format!("{PISTON_API_URL}{path}")
}

// #[inline]
// fn fabric_api_path(path: &str) -> String {
//     format!("{FABRIC_API_URL}{path}")
// }

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
    if let Ok(cached) = CachedResponse::<T>::from_file(&cache_file).await
        && !cached.is_expired()
    {
        let mut msg = "Using cached response".to_string();
        if let Ok(elapsed) = cached.expires.duration_since(SystemTime::now()) {
            let (minutes, seconds) = (elapsed.as_secs() / 60, elapsed.as_secs() % 60);
            let milis = elapsed.subsec_millis();
            write!(msg, " expiring in {minutes:02}:{seconds:02}.{milis:03}")?;
        }
        debug!("{msg}");
        return Ok(cached.data);
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
pub(crate) async fn download_jre(major_version: u8) -> Result<Bytes> {
    let url = format!(
        "https://api.adoptium.net/v3/binary/latest/{feature_version}/{release_type}/{os}/{arch}/{image_type}/{jvm_impl}/{heap_size}/{vendor}",
        feature_version = major_version,
        release_type = "ga",
        os = match std::env::consts::OS {
            "macos" => "mac",
            os => os,
        },
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
        let version = manifest.versions.first().unwrap();
        let metadata = get_version_metadata(version).await.unwrap();
        assert!(metadata.downloads.contains_key("server"));
    }

    #[tokio::test]
    async fn test_download_jre() {
        let version = match std::env::consts::OS {
            "macos" => 11, // Adoptium doesn't have JRE 8 for aarch64 macOS
            _ => 8,
        };

        let mut tries = 0;
        while tries < 3 {
            match download_jre(version).await {
                Ok(jre) => {
                    assert!(!jre.is_empty());
                    break;
                }
                Err(e) => {
                    eprintln!("Failed to download JRE: {e}");
                    tries += 1;
                }
            }
        }
        assert!(tries < 3, "Failed to download JRE after 3 attempts");
    }
}
