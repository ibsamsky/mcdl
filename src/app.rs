use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use color_eyre::eyre::{self, Result, WrapErr, eyre};
use dialoguer::Confirm;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use tokio::fs;
use tokio::process::Command;
use tokio::task::JoinSet;
use tracing::{debug, error, info, instrument, warn};

use crate::cli::WhatEnum;
use crate::jre;
use crate::paths::{LOG_BASE_DIR, PROJ_DIRS};
use crate::types::meta::{InstanceMeta, InstanceSettings, META};
use crate::types::version::{GameVersion, VersionMetadata, VersionNumber};
use crate::utils::net::{REQWEST_CLIENT, get_version_metadata};

static INSTANCE_BASE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJ_DIRS.data_local_dir().join("instance"));
static INSTANCE_SETTINGS_BASE_DIR: LazyLock<PathBuf> =
    LazyLock::new(|| PROJ_DIRS.config_local_dir().join("instance"));
pub(crate) static PB_STYLE: LazyLock<ProgressStyle> = LazyLock::new(|| {
    ProgressStyle::with_template("{prefix:.bold.blue.bright} {spinner:.green.bright} {wide_msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏-")
});

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
        if META!().jre_installed(jre_version) || jres_installed.contains(&jre_version) {
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

        // at the same time, spawn a thread to install the JRE
        let bars = bars.clone();
        let version_id = version.id.clone();
        install_threads.spawn(async move {
            jre::install_jre(jre_version, version_id, Some(bars))
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

    if !META!().jre_installed(jre_version) {
        debug!(jre = jre_version, "Installing JRE due to config change");
        jre::install_jre(jre_version, &id, None).await?;
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
    args.extend(settings.java.args.iter().map(Into::into)); // jvm args
    args.extend(vec!["-jar".into(), settings.server.jar.into()]); // server jar
    args.extend(settings.server.args.iter().map(Into::into)); // server args

    let args_string = args
        .iter()
        .map(|s| shell_escape::escape(s.to_str().unwrap().into()))
        .collect::<Vec<_>>()
        .join(" ");

    let java_path = jre::get_java_path(jre_version);

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
pub(crate) fn locate(what: WhatEnum) -> Result<()> {
    match what {
        WhatEnum::Java => {
            println!("JRE base directory: {}", jre::JRE_BASE_DIR.display());
        }
        WhatEnum::Instance => {
            println!("Instance base directory: {}", INSTANCE_BASE_DIR.display());
        }
        WhatEnum::Config => {
            println!(
                "Instance settings base directory: {}",
                INSTANCE_SETTINGS_BASE_DIR.display()
            );
        }
        WhatEnum::Log => {
            println!("Log base directory: {}", LOG_BASE_DIR.display());
        }
    }

    Ok(())
}
