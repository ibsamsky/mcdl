use std::path::PathBuf;
use std::sync::LazyLock;

use directories::ProjectDirs;

pub(crate) static PROJ_DIRS: LazyLock<ProjectDirs> = LazyLock::new(|| {
    ProjectDirs::from("com.github", "ibsamsky", env!("CARGO_PKG_NAME"))
        .expect("failed to get project directories (no valid home dir)")
});

pub(crate) static LOG_BASE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJ_DIRS.data_local_dir().join("log"));
