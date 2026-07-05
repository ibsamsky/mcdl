use clap::builder::NonEmptyStringValueParser;
use clap::{Args, Parser, Subcommand, ValueEnum};
use color_eyre::eyre::{Result, eyre};
use derive_more::derive::Display;
use tracing::instrument;

use crate::MANIFEST;
use crate::types::version::VersionNumber;

pub(crate) static MCDL_VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    format!(
        "{}{}+g{}",
        env!("CARGO_PKG_VERSION"),
        if option_env!("MCDL_BUILD_DEBUG") == Some("1") {
            "-debug"
        } else {
            ""
        },
        option_env!("MCDL_GIT_SHA").unwrap_or("unknown"),
    )
});

#[doc(hidden)]
#[derive(Parser, Debug)]
#[command(author, version = MCDL_VERSION.as_str())]
#[command(arg_required_else_help = true, subcommand_required = true)]
/// A tool for managing Minecraft server versions
pub(crate) struct Cli {
    #[command(subcommand)]
    pub action: Action,
}

#[doc(hidden)]
#[derive(Subcommand, Debug)]
pub(crate) enum Action {
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
pub(crate) struct ListFilter {
    #[arg(short, long)]
    /// Only list release versions (default)
    pub release: bool,
    #[arg(short, long)]
    /// Only list pre-release versions
    pub pre_release: bool,
    #[arg(short, long)]
    /// Only list snapshot versions
    pub snapshot: bool,
    #[arg(short, long)]
    /// Only list other versions
    pub other: bool,
    #[arg(short, long)]
    /// List all versions
    pub all: bool,
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
pub(crate) enum WhatEnum {
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
    let version = v.parse()?;

    MANIFEST
        .get()
        .expect("manifest not set")
        .versions
        .iter()
        .map(|v| &v.id)
        .find(|v| v == &&version)
        .cloned()
        .map(|_| version)
        .ok_or(eyre!("Version does not exist"))
}
