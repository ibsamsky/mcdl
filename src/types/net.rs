use std::path::Path;
use std::time::SystemTime;

use color_eyre::eyre::Result;
use derive_more::Constructor;
use serde::{Deserialize, Serialize};
use tokio::fs;

#[derive(Serialize, Deserialize, Constructor)]
pub(crate) struct CachedResponse<T> {
    pub data: T,
    pub expires: SystemTime,
}

impl<T> CachedResponse<T> {
    pub fn is_expired(&self) -> bool {
        SystemTime::now() > self.expires
    }

    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self>
    where Self: for<'de> Deserialize<'de> {
        let data = fs::read(path).await?;
        let cached: CachedResponse<T> = rmp_serde::from_slice(&data)?;
        Ok(cached)
    }

    // TODO: make this return type more meaningful
    pub async fn save(&self, path: impl AsRef<Path>) -> Result<()>
    where Self: Serialize {
        let data = rmp_serde::to_vec(self)?;
        fs::create_dir_all(path.as_ref().parent().expect("infallible")).await?;
        fs::write(path, data).await?;
        Ok(())
    }
}
