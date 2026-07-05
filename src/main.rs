//! A tool for managing Minecraft server versions
#![warn(clippy::all, clippy::pedantic, rust_2018_idioms)]

pub(crate) mod app;
pub(crate) mod cli;
pub(crate) mod jre;
pub(crate) mod paths;
pub(crate) mod types;
pub(crate) mod utils;

use std::fs::File;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};
use color_eyre::eyre::{Result, WrapErr, eyre};
use color_eyre::owo_colors::OwoColorize;
use prettytable::format::FormatBuilder;
use prettytable::{Cell, Row, Table, row};
use tracing::{debug, info, instrument};

use crate::cli::{Action, Cli, ListFilter, WhatEnum};
use crate::paths::{LOG_BASE_DIR, PROJ_DIRS};
use crate::types::meta::{META, ToArgs};
use crate::types::version::{GameVersionList, VersionNumber};
use crate::utils::net::get_version_manifest;

pub(crate) static MANIFEST: OnceLock<GameVersionList> = OnceLock::new();

/* main */

#[instrument(err(Debug), ret)]
#[tokio::main]
async fn main() -> Result<()> {
    // install color_eyre before any fallible startup code can create an `eyre::Report`
    #[cfg(not(test))]
    color_eyre::config::HookBuilder::default()
        .display_env_section(true)
        .theme(color_eyre::config::Theme::new())
        .install()?;

    MANIFEST
        .set(get_version_manifest().await?)
        .map_err(|_| unreachable!("manifest already set"))?;

    let args = std::env::args().collect::<Vec<_>>();

    let log_name = format!(
        "mcdl-{}{}.log",
        Utc::now().format("%Y%m%d-%H%M%S"),
        if args.len() > 1 {
            format!("-{}", args[1])
        } else {
            String::new()
        }
    );
    let log_path = LOG_BASE_DIR.join(log_name);

    // set up tracing
    install_tracing(&log_path)?;
    info!("Logging to {}", log_path.display());

    info!("Args: {}", args.to_args_string());

    // lol again
    let cli = tokio::task::spawn_blocking(Cli::parse).await?;
    debug!(?cli);

    match cli.action {
        Action::List { filter, installed } => list_impl(filter, installed).await?,
        Action::Info { version } => info_impl(version).await?,
        Action::Install { version } => install_impl(version).await?,
        Action::Uninstall { version } => uninstall_impl(version)?,
        Action::Run { version } => run_impl(version).await?,
        Action::Locate { what } => locate_impl(what)?,
    }

    Ok(())
}

fn install_tracing(path: &PathBuf) -> Result<()> {
    use tracing_error::ErrorLayer;
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{EnvFilter, fmt};

    std::fs::create_dir_all(LOG_BASE_DIR.as_path())?;
    let file = File::create(path)?;

    let fmt_layer = fmt::layer()
        .with_ansi(false)
        // .with_timer(fmt::time::uptime())
        .with_thread_ids(true)
        .with_writer(Mutex::new(file));
    let filter_layer =
        EnvFilter::try_from_default_env().or_else(|_| EnvFilter::try_new("mcdl=debug"))?;

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();

    Ok(())
}

/* end main */

/* impls */

#[instrument(err, ret(level = "debug"), skip(filter))]
async fn list_impl(filter: Option<ListFilter>, installed: bool) -> Result<()> {
    let filter = filter.unwrap_or_default();
    debug!(?filter);

    let mut versions = MANIFEST
        .get()
        .expect("manifest not set")
        .versions
        .iter()
        .filter(|v| {
            match (
                filter.release,
                filter.pre_release,
                filter.snapshot,
                filter.other,
                filter.all,
            ) {
                (true, _, _, _, _) => v.id.is_release(),
                (_, true, _, _, _) => v.id.is_pre_release(),
                (_, _, true, _, _) => v.id.is_snapshot(),
                (_, _, _, true, _) => v.id.is_other(),
                (_, _, _, _, true) => true,
                _ => unreachable!(),
            }
        })
        .collect::<Vec<_>>();
    versions.sort();

    info!("Found {} matching versions", versions.len());

    if installed {
        // installed versions only, more info
        info!("Filtering for installed versions");

        let installed_instances = &META.lock().instances;
        let filtered_instances = installed_instances
            .iter()
            .filter(|(_, i)| versions.iter().any(|v| v.id == i.id))
            .collect::<Vec<_>>();

        info!("Found {} installed versions", filtered_instances.len());
        if filtered_instances.is_empty() {
            println!("No matching versions installed");
            return Ok(());
        }

        let mut table = Table::new();
        table.set_format(
            FormatBuilder::new()
                .column_separator(' ')
                .borders(' ')
                .padding(1, 1)
                .build(),
        );

        table.set_titles(row![b => "ID", "Version", "Type", "JRE"]);

        for (id, instance) in filtered_instances {
            let version = versions.iter().find(|v| v.id == instance.id).unwrap();
            let location = PROJ_DIRS.data_local_dir().join("instance").join(id);

            table.add_row(row![id, version.id, version.release_type, instance.jre]);
            table.add_row(row![H4->format!("{} {}", "Location:".bold(), location.display())]);
            table.add_empty_row();
        }

        table.printstd();
    } else {
        // short info for all versions
        info!("Filtering for all versions");

        if !std::io::stdout().is_terminal() {
            for v in versions {
                println!("{}", v.id);
            }
            return Ok(());
        }

        let mut table = Table::new();
        table.set_format(
            FormatBuilder::new()
                .column_separator(' ')
                .borders(' ')
                .padding(1, 1)
                .build(),
        );

        table.set_titles(row![b => "Version", "Type", "Release Date"]);
        for version in versions {
            table.add_row(Row::new(vec![
                Cell::new(&version.id.to_string()),
                Cell::new(&version.release_type.clone()).style_spec(
                    match version.release_type.as_str() {
                        "release" => "Fgb",
                        _ => "",
                    },
                ),
                Cell::new(&version.release_time.to_string()),
            ]));
        }

        table.printstd();
    }

    Ok(())
}

#[instrument(err, ret(level = "debug"))]
async fn info_impl(version: VersionNumber) -> Result<()> {
    let version = MANIFEST
        .get()
        .expect("manifest not set")
        .versions
        .iter()
        .find(|v| v.id == version)
        .expect("infallible");

    let time_format = "%-d %B %Y at %-I:%M:%S%P UTC";
    let message = format!(
        "Version {} ({})\nReleased: {}\nLast updated: {}",
        version.id,
        version.release_type,
        version.release_time.format(time_format),
        version.time.format(time_format),
    );

    println!("{message}");

    Ok(())
}

#[instrument(err, ret(level = "debug"), skip(versions))]
async fn install_impl(versions: Option<Vec<VersionNumber>>) -> Result<()> {
    let manifest = MANIFEST.get().expect("manifest not set");
    let game_versions = &manifest.versions;
    let latest = &manifest.latest;

    if versions.is_none() {
        println!("Installing latest release version\n");
        let latest = game_versions
            .iter()
            .find(|v| v.id == latest.release)
            .ok_or_else(|| eyre!("No latest release version found"))?;
        app::install_versions(vec![latest])
            .await
            .wrap_err("Error while installing latest version")?;

        return Ok(());
    }

    let versions = versions.unwrap();
    if versions.is_empty() {
        Cli::command()
            .error(ErrorKind::ValueValidation, "No version provided")
            .exit();
    }

    println!(
        "Installing {} version{}: {}\n",
        versions.len(),
        if versions.len() == 1 { "" } else { "s" },
        versions
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    );

    let to_install_versions = game_versions
        .iter()
        .filter(|v| versions.contains(&v.id))
        .collect::<Vec<_>>();
    app::install_versions(to_install_versions)
        .await
        .wrap_err("Error while installing versions")?;

    Ok(())
}

#[instrument(err, ret(level = "debug"))]
fn uninstall_impl(version: String) -> Result<()> {
    app::uninstall_instance(version.parse()?).wrap_err("Error while uninstalling instance")?;

    Ok(())
}

#[instrument(err, ret(level = "debug"))]
async fn run_impl(version: String) -> Result<()> {
    app::run_instance(version.parse()?)
        .await
        .wrap_err("Error while running server")?;

    Ok(())
}

#[instrument(err, ret(level = "debug"))]
fn locate_impl(what: WhatEnum) -> Result<()> {
    app::locate(what).wrap_err(format!("Error while locating `{what}`"))?;

    Ok(())
}

/* end impls */
