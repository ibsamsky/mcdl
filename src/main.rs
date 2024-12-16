//! A tool for managing Minecraft server versions

pub(crate) mod app;
pub(crate) mod common;
pub(crate) mod types;
pub(crate) mod utils;

use std::fs::File;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::Mutex;

use async_once::AsyncOnce;
use chrono::Utc;
use clap::builder::NonEmptyStringValueParser;
use clap::error::ErrorKind;
use clap::{arg, command, Args, CommandFactory, Parser, Subcommand, ValueEnum};
use color_eyre::config::{HookBuilder, Theme};
use color_eyre::eyre::{eyre, Result, WrapErr};
use color_eyre::owo_colors::OwoColorize;
use derive_more::derive::Display;
use itertools::Itertools;
use lazy_static::lazy_static;
use prettytable::format::FormatBuilder;
use prettytable::{row, Cell, Row, Table};
use tracing::{debug, info, instrument};

use crate::common::{LOG_BASE_DIR, MCDL_VERSION, META, PROJ_DIRS};
use crate::types::meta::AsArgs;
use crate::types::version::{GameVersionList, VersionNumber};
use crate::utils::net::get_version_manifest;

lazy_static! {
    static ref MANIFEST: AsyncOnce<GameVersionList> = AsyncOnce::new(async {
        get_version_manifest()
            .await
            .expect("Failed to get version manifest")
    });
}

/* cli */

#[doc(hidden)]
#[derive(Parser, Debug)]
#[command(author, version = MCDL_VERSION.as_str())]
#[command(arg_required_else_help = true, subcommand_required = true)]
/// A tool for managing Minecraft server versions
struct Cli {
    #[command(subcommand)]
    action: Action,
}

#[doc(hidden)]
#[derive(Subcommand, Debug)]
enum Action {
    /// List available Minecraft versions
    List {
        #[command(flatten)]
        filter: Option<ListFilter>,
        #[arg(short, long)]
        /// List installed instances and their versions
        installed: bool,
    },
    /// Get information about a Minecraft version
    Info {
        #[arg(required = true, value_parser = |s: &str| validate_version_number(s))]
        #[arg(short, long)]
        /// The Minecraft version to get information about
        version: VersionNumber,
    },
    /// Install a server instance
    Install {
        #[arg(value_delimiter = ',', num_args = 0.., value_parser = |s: &str| validate_version_number(s))]
        #[arg(short, long)]
        /// The version(s) to install
        ///
        /// Defaults to latest release version if none is provided.
        /// Can be specified multiple times, or as a comma or space-separated list.
        version: Option<Vec<VersionNumber>>,
        // #[arg(short, long)]
        // name: Option<String>,
    },
    /// Uninstall a server instance
    Uninstall {
        #[arg(required = true, value_parser = NonEmptyStringValueParser::new())]
        #[arg(short, long)]
        version: String, // in the future, `name` will be used instead
    },
    /// Run a server instance
    Run {
        #[arg(required = true, value_parser = NonEmptyStringValueParser::new())]
        #[arg(short, long)]
        /// The version to run
        version: String, // in the future, `name` will be used instead
    },
    /// Print the path to a config file or instance directory
    Locate {
        #[arg(required = true)]
        #[arg(value_enum)]
        /// The file or directory to locate
        what: WhatEnum,
    },
}

#[doc(hidden)]
#[derive(Args, Debug)]
#[group(id = "filter", required = false, multiple = false)]
struct ListFilter {
    #[arg(short, long)]
    /// Only list release versions (default)
    release: bool,
    #[arg(short, long)]
    /// Only list pre-release versions
    pre_release: bool,
    #[arg(short, long)]
    /// Only list snapshot versions
    snapshot: bool,
    #[arg(short, long)]
    /// Only list other versions
    other: bool,
    #[arg(short, long)]
    /// List all versions
    all: bool,
}

impl Default for ListFilter {
    fn default() -> Self {
        Self {
            release: true,
            pre_release: false,
            snapshot: false,
            other: false,
            all: false,
        }
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, ValueEnum, Debug, Display)]
enum WhatEnum {
    /// The Java Runtime Environment directory
    Java,
    /// The directory containing Minecraft server instances
    Instance,
    /// The directory containing configuration files
    Config,
    /// The directory containing logs
    Log,
}

#[instrument(level = "debug", err, ret)]
fn validate_version_number(v: &str) -> Result<VersionNumber> {
    // lol
    let valid_versions: Vec<VersionNumber> =
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            MANIFEST
                .get()
                .await
                .versions
                .iter()
                .map(|v| v.id.clone())
                .collect()
        });

    let version = v.parse()?;

    if valid_versions.contains(&version) {
        Ok(version)
    } else {
        Err(eyre!("Version does not exist"))
    }
}

/* end cli */

/* main */

#[instrument(err(Debug), ret)]
#[tokio::main]
async fn main() -> Result<()> {
    let args = std::env::args().collect_vec();

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

    // install color_eyre
    HookBuilder::default()
        .display_env_section(true)
        .theme(Theme::new())
        .install()?;

    info!("Args: {}", args.as_args_string());

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
    use tracing_subscriber::{fmt, EnvFilter};

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

    let versions = MANIFEST
        .get()
        .await
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
        .sorted()
        .collect_vec();

    info!("Found {} matching versions", versions.len());

    if installed {
        // installed versions only, more info
        info!("Filtering for installed versions");

        let installed_instances = &META.lock().instances;
        let filtered_instances = installed_instances
            .iter()
            .filter(|(_, i)| versions.iter().any(|v| v.id == i.id))
            .collect_vec();

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
                Cell::new(&version.release_type.to_string()).style_spec(
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
        .await
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
    let manifest = MANIFEST.get().await;
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
        versions.iter().map(ToString::to_string).join(", ")
    );

    let to_install_versions = game_versions
        .iter()
        .filter(|v| versions.contains(&v.id))
        .collect_vec();
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
    // TODO: pass directly
    app::locate(&what.to_string()).wrap_err(format!("Error while locating `{what}`"))?;

    Ok(())
}

/* end impls */
