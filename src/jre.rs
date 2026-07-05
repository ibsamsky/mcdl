use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use bytes::Bytes;
use color_eyre::eyre::{Result, WrapErr, eyre};
use indicatif::{MultiProgress, ProgressBar};
use tracing::{debug, info, instrument};

use crate::app::PB_STYLE;
use crate::paths::PROJ_DIRS;
use crate::types::meta::META;
use crate::utils::net::download_jre;

pub(crate) static JRE_BASE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJ_DIRS.data_local_dir().join("jre"));

macro_rules! META {
    () => {
        META.clone().lock()
    };
}

#[instrument(err, ret(level = "debug"), skip(label, progress))]
pub(crate) async fn install_jre(
    major_version: u8,
    label: impl std::fmt::Display,
    progress: Option<MultiProgress>,
) -> Result<()> {
    let jre_dir = JRE_BASE_DIR.join(major_version.to_string());
    let pb = progress_bar(major_version, label, progress);

    if META!().jre_installed(major_version) {
        pb.finish_with_message("Cancelled (already installed)");
        debug!("Cancelled JRE install (this should never happen)");
        return Ok(());
    }

    pb.set_message("Downloading JRE...");
    info!("Starting JRE download");
    let jre = download_jre(major_version).await?;
    info!("Downloaded JRE");

    pb.set_message("Extracting JRE...");
    info!("Starting JRE extraction");
    extract_jre(jre, &jre_dir).wrap_err("Failed to extract JRE")?;
    info!("Extracted JRE");

    pb.set_message("Updating metadata...");
    META!().add_jre(major_version);
    META!().save()?;

    pb.finish_with_message("Done!");
    info!("Installed JRE");
    Ok(())
}

fn progress_bar(
    version: u8,
    label: impl std::fmt::Display,
    progress: Option<MultiProgress>,
) -> ProgressBar {
    let pb = ProgressBar::new_spinner()
        .with_style(PB_STYLE.clone())
        .with_prefix(format!("JRE {version} for {label}"));
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    progress.map_or(pb.clone(), |progress| progress.add(pb))
}

#[cfg(windows)]
#[instrument(err, ret(level = "debug"), skip_all, fields(path = %jre_dir.as_ref().display()))]
fn extract_jre(jre: Bytes, jre_dir: impl AsRef<Path>) -> Result<()> {
    use std::io::{BufReader, Cursor};

    use zip::ZipArchive;

    let jre_dir = jre_dir.as_ref();

    std::fs::create_dir_all(jre_dir).wrap_err(format!(
        "Failed to create directory for JRE: {path}",
        path = jre_dir.display()
    ))?;

    let reader: BufReader<Cursor<Vec<u8>>> = BufReader::new(Cursor::new(jre.into()));
    let mut archive = ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let filepath = entry.enclosed_name().ok_or(eyre!("Invalid file path"))?;
        let outpath = jre_dir.join(filepath.components().skip(1).collect::<PathBuf>());

        if entry.is_dir() {
            if outpath.exists() {
                tracing::warn!(path = %outpath.display(), "Clobbering existing file");
            }
            std::fs::create_dir_all(outpath)?;
            continue;
        }

        let mut outfile = std::fs::File::create(&outpath)?;
        std::io::copy(&mut entry, &mut outfile)?;
    }

    let java_path = jre_dir.join("bin").join("java.exe");
    if !java_path.exists() {
        return Err(eyre!(
            "Failed to extract JRE ({} does not exist)",
            java_path.display()
        ));
    }

    Ok(())
}

#[cfg(target_os = "linux")]
#[instrument(err, ret(level = "debug"), skip_all, fields(path = %jre_dir.as_ref().display()))]
fn extract_jre(jre: Bytes, jre_dir: impl AsRef<Path>) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    use bytes::Buf;
    use flate2::read::GzDecoder;
    use tar::Archive;

    let mut reader = jre.reader();
    let mut archive = Archive::new(GzDecoder::new(&mut reader));
    let entries = archive.entries()?;
    let jre_dir = jre_dir.as_ref();

    std::fs::create_dir_all(jre_dir).wrap_err(format!(
        "Failed to create directory for JRE: {path}",
        path = jre_dir.display()
    ))?;

    for entry in entries {
        let mut entry = entry?;
        let filepath = entry.path()?;
        let outpath = jre_dir.join(filepath.components().skip(1).collect::<PathBuf>());

        entry.unpack(outpath)?;
    }

    let java_path = jre_dir.join("bin").join("java");
    if !java_path.exists() {
        return Err(eyre!(
            "Failed to extract JRE ({} does not exist)",
            java_path.display()
        ));
    }

    let mut perms = std::fs::metadata(&java_path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&java_path, perms)?;

    Ok(())
}

#[cfg(not(any(windows, target_os = "linux")))]
#[instrument(err, ret(level = "debug"), skip(_jre))]
fn extract_jre(_jre: Bytes, _jre_dir: &PathBuf) -> Result<()> {
    Err(eyre!("Unsupported OS")) // TODO fail gracefully
}

#[instrument(ret(level = "debug"))]
pub(crate) fn get_java_path(version: u8) -> PathBuf {
    JRE_BASE_DIR
        .join(version.to_string())
        .join("bin")
        .join(format!("java{}", std::env::consts::EXE_SUFFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn test_install_jre() {
        let version = match std::env::consts::OS {
            "macos" => 11,
            _ => 8,
        };

        scopeguard::defer! {
            let path = JRE_BASE_DIR.join(version.to_string());

            if path.exists() {
                std::fs::remove_dir_all(path).unwrap();
            }

            META!().remove_jre(version);
            META!().save().unwrap();
        }

        assert!(
            !META!().jre_installed(version),
            "JRE 8 is already installed"
        );

        install_jre(version, version, None).await.unwrap();

        assert!(
            get_java_path(version).exists(),
            "{:?} does not exist",
            get_java_path(version)
        );
        assert!(META!().remove_jre(version), "Failed to remove JRE");
        assert!(META!().save().is_ok(), "Failed to save metadata");
    }
}
