use lazy_static::lazy_static;

lazy_static! {
    pub static ref MCDL_VERSION: String = {
        format!(
            "{}{}+g{}",
            env!("CARGO_PKG_VERSION"),
            match env!("VERGEN_CARGO_OPT_LEVEL") {
                "1" => "-debug",
                _ => "",
            },
            env!("VERGEN_GIT_SHA"),
        )
    };
    pub static ref REQWEST_CLIENT: reqwest::Client = {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_str(&format!(
                "mcdl/{} ({})",
                MCDL_VERSION.as_str(),
                env!("CARGO_PKG_HOMEPAGE")
            ))
            .expect("failed to build user agent header"),
        );

        reqwest::Client::builder()
            .default_headers(headers)
            .tcp_keepalive(Some(std::time::Duration::from_secs(10)))
            .build()
            .expect("failed to build reqwest client")
    };
    pub static ref PROJ_DIRS: directories::ProjectDirs =
        directories::ProjectDirs::from("com.github", "paenis", env!("CARGO_PKG_NAME"))
            .expect("failed to get project directories");
    pub static ref LOG_BASE_DIR: std::path::PathBuf = PROJ_DIRS.data_local_dir().join("log");
    static ref META_PATH: std::path::PathBuf = PROJ_DIRS.data_local_dir().join("meta.mpk");
    pub(crate) static ref META: std::sync::Arc<parking_lot::Mutex<crate::types::meta::AppMeta>> =
        std::sync::Arc::new(parking_lot::Mutex::new(
            crate::types::meta::AppMeta::read_or_create(META_PATH.as_path())
        ));
}
