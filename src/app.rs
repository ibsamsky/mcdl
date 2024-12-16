use std::borrow::Cow;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use bytes::Bytes;
use color_eyre::eyre::{self, eyre, Result, WrapErr};
use dialoguer::Confirm;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use itertools::Itertools;
use lazy_static::lazy_static;
use tokio::fs;
use tokio::process::Command;
use tokio::task::JoinSet;
use tracing::{debug, error, info, instrument, warn};

use crate::common::{LOG_BASE_DIR, META, PROJ_DIRS, REQWEST_CLIENT};
use crate::types::meta::{InstanceMeta, InstanceSettings};
use crate::types::version::{GameVersion, VersionMetadata, VersionNumber};
use crate::utils::net::{download_jre, get_version_metadata};

lazy_static! {
    static ref INSTANCE_BASE_DIR: PathBuf = PROJ_DIRS.data_local_dir().join("instance");
    static ref JRE_BASE_DIR: PathBuf = PROJ_DIRS.data_local_dir().join("jre");
    static ref INSTANCE_SETTINGS_BASE_DIR: PathBuf = PROJ_DIRS.config_local_dir().join("instance");
    static ref PB_STYLE: ProgressStyle = ProgressStyle::with_template(
        "{prefix:.bold.blue.bright} {spinner:.green.bright} {wide_msg}",
    )
    .unwrap()
    .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏-");
}

macro_rules! META {
    () => {
        META.clone().lock()
    };
}

// ideally there is one public function for each subcommand

#[instrument(err, ret(level = "debug"), skip(versions))]
pub(crate) async fn install_versions(versions: Vec<&GameVersion>) -> Result<()> {
    info!("Installing {} versions", versions.len());

    let mut install_threads = JoinSet::new();
    let bars = MultiProgress::new();

    let mut jres_installed: Vec<u8> = Vec::new();

    for version in versions {
        let version_display = version.id.to_string();
        debug!(version = version_display, version.url, "Entering loop");

        let cloned_meta = META.clone();
        let pb_server = bars.add(
            ProgressBar::new_spinner()
                .with_style(PB_STYLE.clone())
                .with_prefix(version.id.to_string()),
        );
        pb_server.enable_steady_tick(Duration::from_millis(100));

        pb_server.set_message("Getting version metadata...");
        let version_meta: VersionMetadata = get_version_metadata(version).await?;
        let jre_version = version_meta.java_version.major_version;

        // spawn a thread to install the version
        let thread_version_display = version_meta.id.to_string();
        install_threads.spawn(async move {
            debug!(version = thread_version_display, "Entering install thread");

            if !version_meta.downloads.contains_key("server") {
                pb_server.finish_with_message("Cancelled (no server jar)");
                debug!(
                    version = thread_version_display,
                    "Exiting install thread (no server jar)"
                );
                return Ok::<(), eyre::Report>(());
            }

            let instance_dir = INSTANCE_BASE_DIR.join(version_meta.id.to_string());

            // only necessary while there is one instance per version
            if META.lock().instance_installed(&version_meta.id.to_string()) {
                pb_server.finish_with_message("Cancelled (already installed)");
                debug!(
                    version = thread_version_display,
                    "Exiting install thread (already installed)"
                );
                return Ok::<(), eyre::Report>(());
            }

            let url = version_meta
                .downloads
                .get("server")
                .expect("infallible")
                .url
                .clone();

            pb_server.set_message("Downloading server jar...");
            let server_jar = REQWEST_CLIENT
                .get(url)
                .send()
                .await
                .wrap_err("Failed to download server jar")?
                .bytes()
                .await
                .wrap_err("Failed to read server jar to bytes")?;

            // write to disk
            pb_server.set_message("Writing server jar to disk...");
            fs::create_dir_all(&instance_dir).await.wrap_err(format!(
                "Failed to create instance directory for {}",
                version_meta.id
            ))?;

            fs::write(instance_dir.join("server.jar"), server_jar)
                .await
                .wrap_err(format!(
                    "Failed to write server jar for {}",
                    version_meta.id
                ))?;

            // write eula
            pb_server.set_message("Writing eula.txt...");
            fs::write(instance_dir.join("eula.txt"), "eula=true")
                .await
                .wrap_err(format!("Failed to write eula.txt for {}", version_meta.id))?;

            // write settings
            pb_server.set_message("Writing settings...");
            let settings = InstanceSettings::new(jre_version);
            let settings_path =
                INSTANCE_SETTINGS_BASE_DIR.join(format!("{}.toml", version_meta.id));

            settings.save(&settings_path).await?;

            // update meta
            pb_server.set_message("Updating metadata...");
            let mut instance_meta = InstanceMeta::new(version_meta.id, jre_version);
            instance_meta.add_file(&instance_dir);
            instance_meta.add_file(&settings_path);

            let mut meta = cloned_meta.lock();
            meta.add_instance(instance_meta);
            meta.save()?;

            pb_server.finish_with_message("Done!");

            info!(version = thread_version_display, "Installed version");
            debug!(version = thread_version_display, "Exiting install thread");
            Ok::<(), eyre::Report>(())
        });

        // if the JRE is already installed, skip it
        if META!().jre_installed(&jre_version) || jres_installed.contains(&jre_version) {
            debug!(
                jre = jre_version,
                version = version_display,
                "Skipping JRE install"
            );
            continue;
        }

        // otherwise, install it
        jres_installed.push(jre_version);

        info!(
            jre = jre_version,
            version = version_display,
            "Installing JRE"
        );

        let pb_jre = bars.add(
            ProgressBar::new_spinner()
                .with_style(PB_STYLE.clone())
                .with_prefix(format!("JRE {jre_version} for {}", version.id)),
        );
        pb_jre.enable_steady_tick(Duration::from_millis(100));

        // at the same time, spawn a thread to install the JRE
        install_threads.spawn(async move {
            pb_jre.set_message("Installing JRE...");
            install_jre(&jre_version, &pb_jre)
                .await
                .wrap_err(format!("Failed to install JRE {jre_version}"))?;

            Ok::<(), eyre::Report>(())
        });

        debug!(version = version_display, version.url, "Exiting loop");
    }

    while let Some(result) = install_threads.join_next().await {
        result?.wrap_err("Failed to install server or JRE")?;
    }

    Ok(())
}

// pub(crate) async fn install_version(version: &GameVersion) -> Result<()> {
//     install_versions(vec![version]).await
// }

#[instrument(err, ret(level = "debug"), skip(pb))]
async fn install_jre(major_version: &u8, pb: &ProgressBar) -> Result<()> {
    let jre_dir = JRE_BASE_DIR.join(major_version.to_string());

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
    META!().add_jre(*major_version);
    META!().save()?;

    pb.finish_with_message("Done!");
    info!("Installed JRE");
    Ok(())
}

#[instrument(err, ret(level = "debug"), skip(id))]
pub(crate) fn uninstall_instance(id: VersionNumber) -> Result<()> {
    let pb = ProgressBar::new_spinner()
        .with_style(PB_STYLE.clone())
        .with_prefix(id.to_string());
    pb.enable_steady_tick(Duration::from_millis(100));

    let mut instance_files = vec![];

    pb.set_message("Checking if instance exists...");
    if let Some(instance) = META!().instances.get(&id.to_string()) {
        instance_files.extend(instance.files.clone());
    } else {
        return Err(eyre!("Instance `{id}` does not exist"));
    }

    pb.set_message("Removing files...");
    for path in &instance_files {
        if !path.exists() {
            warn!(?path, "File does not exist");
            continue;
        }

        if path.is_dir() {
            info!(?path, "Removing directory");
            std::fs::remove_dir_all(path)
                .wrap_err(format!("Failed to remove directory {}", path.display()))?;
        } else {
            info!(?path, "Removing file");
            std::fs::remove_file(path)
                .wrap_err(format!("Failed to remove file {}", path.display()))?;
        }

        META!()
            .instances
            .get_mut(&id.to_string())
            .unwrap()
            .remove_file(path);
        META!().save()?;
    }

    pb.set_message("Updating metadata...");
    META!().remove_instance(&id.to_string());
    META!().save()?;

    // bonus: remove jre if it's not used by any other instances

    pb.finish_with_message("Done!");
    Ok(())
}

#[instrument(err, ret(level = "debug"), skip(id))]
pub(crate) async fn run_instance(id: VersionNumber) -> Result<()> {
    let instance_path = INSTANCE_BASE_DIR.join(id.to_string());

    if !META!().instance_installed(&id.to_string()) {
        return Err(eyre!("Instance `{id}` does not exist"));
    }

    let settings =
        InstanceSettings::from_file(INSTANCE_SETTINGS_BASE_DIR.join(format!("{id}.toml"))).await?;
    debug!(?settings, "Loaded instance settings");

    // check if the JRE is installed and install it if not
    let jre_version = settings.java.version;

    if !META!().jre_installed(&jre_version) {
        debug!(jre = jre_version, "Installing JRE due to config change");
        let pb = ProgressBar::new_spinner()
            .with_style(PB_STYLE.clone())
            .with_prefix(format!("JRE {jre_version} for {id}"));
        pb.enable_steady_tick(Duration::from_millis(100));

        install_jre(&jre_version, &pb).await?;
    }

    // make sure JRE version is correct
    META!()
        .instances
        .get_mut(&id.to_string())
        .ok_or_else(|| eyre!("Instance metadata not found for {id}"))?
        .jre = jre_version;
    META!().save()?;

    // add all arguments
    let mut args: Vec<OsString> = vec![];
    args.extend(settings.java.args.iter().map(|s| s.into())); // jvm args
    args.extend(vec!["-jar".into(), settings.server.jar.into()]); // server jar
    args.extend(settings.server.args.iter().map(|s| s.into())); // server args

    let args_string = args
        .iter()
        .map(|s| shell_escape::escape(Cow::Borrowed(s.to_str().unwrap())))
        .join(" ");

    let java_path = get_java_path(jre_version);

    debug!(
        "Starting server with command line: {java} {args}",
        java = java_path.display(),
        args = args_string
    );
    let mut child = Command::new(&java_path)
        .current_dir(&instance_path)
        .kill_on_drop(true)
        .args(&args)
        .spawn()
        .wrap_err(format!(
            "Failed to start server with command line: {java} {args}",
            java = java_path.display(),
            args = args_string
        ))?;
    info!("Started server");

    let status = child.wait().await.wrap_err("Failed to wait for server")?;
    if !status.success() {
        error!(?status, "Server exited with an error");
        let upload = Confirm::new()
            .with_prompt("Server exited with an error. Would you like to upload the crash report?")
            .default(false)
            .interact()?;

        if upload {
            debug!("Uploading crash report");
            let crash_reports = instance_path.join("crash-reports");

            let latest = std::fs::read_dir(crash_reports)
                .wrap_err("Failed to read crash reports directory")?
                .filter_map(Result::ok)
                .max_by(|a, b| {
                    let a = a.metadata().unwrap().modified().unwrap();
                    let b = b.metadata().unwrap().modified().unwrap();

                    a.cmp(&b)
                })
                .ok_or_else(|| eyre!("No crash reports found"))?;

            let content =
                std::fs::read_to_string(latest.path()).wrap_err("Failed to read crash report")?;

            // upload to mclo.gs
            let response = REQWEST_CLIENT
                .post("https://api.mclo.gs/1/log")
                .form(&[("content", content)])
                .send()
                .await?;

            // parse json response
            let response: serde_json::Value = response.json().await?;

            if response["success"].as_bool().unwrap() {
                println!(
                    "Crash report uploaded to {}",
                    response["url"].as_str().unwrap()
                );
                debug!(
                    url = response["url"].as_str().unwrap(),
                    "Crash report uploaded"
                );
            } else {
                return Err(eyre!(
                    "Failed to upload crash report: {}",
                    response["error"].as_str().unwrap()
                ));
            }
        }

        return Err(eyre!(
            "Server exited with {status}. Command line: {java} {args}",
            java = java_path.display(),
            args = args_string
        ));
    }

    Ok(())
}

#[instrument(err, ret(level = "debug"))]
pub(crate) fn locate(what: &String) -> Result<()> {
    match what.to_ascii_lowercase().as_str() {
        "java" => {
            println!("JRE base directory: {}", JRE_BASE_DIR.display());
        }
        "instance" => {
            println!("Instance base directory: {}", INSTANCE_BASE_DIR.display());
        }
        "config" => {
            println!(
                "Instance settings base directory: {}",
                INSTANCE_SETTINGS_BASE_DIR.display()
            );
        }
        "log" => {
            println!("Log base directory: {}", LOG_BASE_DIR.display());
        }
        _ => {
            return Err(eyre!("Unknown location: {what}"));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[cfg(not(target_os = "macos"))]
    async fn test_install_jre() {
        let version = match std::env::consts::OS {
            "macos" => 11, // Adoptium doesn't have JRE 8 for aarch64 macOS
            _ => 8,
        };

        // remove the jre directory if the test panics
        scopeguard::defer! {
            let path = JRE_BASE_DIR.join(version.to_string());

            if path.exists() {
                std::fs::remove_dir_all(path).unwrap();
            }

            META!().remove_jre(&version);
            META!().save().unwrap();
        }

        assert!(
            !META!().jre_installed(&version),
            "JRE 8 is already installed"
        );

        install_jre(&version, &ProgressBar::hidden()).await.unwrap();

        assert!(
            get_java_path(version.clone()).exists(),
            "{:?} does not exist",
            get_java_path(8)
        );
        assert!(META!().remove_jre(&version), "Failed to remove JRE");
        assert!(META!().save().is_ok(), "Failed to save metadata");
    }
}

// platform specific stuff

#[cfg(windows)]
#[instrument(err, ret(level = "debug"), skip(jre))]
fn extract_jre(jre: Bytes, jre_dir: &PathBuf) -> Result<()> {
    use std::io::{BufReader, Cursor, Read};

    use zip::ZipArchive;

    std::fs::create_dir_all(jre_dir).wrap_err(format!(
        "Failed to create directory for JRE: {path}",
        path = jre_dir.display()
    ))?;

    let reader: BufReader<Cursor<Vec<u8>>> = BufReader::new(Cursor::new(jre.into()));
    let mut archive = ZipArchive::new(reader)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let path = entry.enclosed_name().unwrap();

        // strip the first directory
        let path: PathBuf = path.components().skip(1).collect();
        let path = jre_dir.join(path);

        if entry.is_dir() {
            std::fs::create_dir_all(path)?;
            continue;
        }

        let mut buf = vec![0u8; entry.size() as usize];
        entry.read_exact(&mut buf)?;

        std::fs::write(path, buf)?;
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
#[instrument(err, ret(level = "debug"), skip(jre))]
fn extract_jre(jre: Bytes, jre_dir: &PathBuf) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    use bytes::Buf;
    use flate2::read::GzDecoder;
    use tar::Archive;

    let mut reader = jre.reader();
    let mut archive = Archive::new(GzDecoder::new(&mut reader));
    let entries = archive.entries()?;

    std::fs::create_dir_all(jre_dir).wrap_err(format!(
        "Failed to create directory for JRE: {path}",
        path = jre_dir.display()
    ))?;

    for entry in entries {
        let mut entry = entry?;
        let path = entry.path()?;

        // strip the first directory
        let path: PathBuf = path.components().skip(1).collect();
        let path = jre_dir.join(path);

        entry.unpack(path)?;
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
fn get_java_path(version: u8) -> PathBuf {
    JRE_BASE_DIR
        .join(version.to_string())
        .join("bin")
        .join(format!("java{}", std::env::consts::EXE_SUFFIX))
}
