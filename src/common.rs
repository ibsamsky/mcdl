use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use directories::ProjectDirs;
use parking_lot::Mutex;
use reqwest::Client;
use reqwest::header::{self, HeaderMap};

use crate::types::meta::AppMeta;

pub static MCDL_VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}{}+g{}",
        env!("CARGO_PKG_VERSION"),
        match env!("VERGEN_CARGO_OPT_LEVEL") {
            "1" => "-debug",
            _ => "",
        },
        env!("VERGEN_GIT_SHA"),
    )
});

pub static REQWEST_CLIENT: LazyLock<Client> = LazyLock::new(|| {
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

pub static PROJ_DIRS: LazyLock<ProjectDirs> = LazyLock::new(|| {
    ProjectDirs::from("com.github", "ibsamsky", env!("CARGO_PKG_NAME"))
        .expect("failed to get project directories (no valid home dir)")
});

pub static LOG_BASE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJ_DIRS.data_local_dir().join("log"));

pub static META: LazyLock<Arc<Mutex<AppMeta>>> = LazyLock::new(|| {
    Arc::new(Mutex::new(AppMeta::read_or_create(
        PROJ_DIRS.data_local_dir().join("meta.mpk").as_path(),
    )))
});
