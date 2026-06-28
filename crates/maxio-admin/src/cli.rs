use crate::commands;
use crate::config::ConfigFile;
use crate::error::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "maxio-admin",
    about = "Remote administration CLI for MaxIO object storage",
    version = maxio::version::VERSION,
    after_help = "Remote commands require a profile (see `maxio-admin config path`). \
                  Local-only commands accept --data-dir explicitly."
)]
pub struct Cli {
    /// Profile name from config (see `config path` for file location)
    #[arg(long, global = true, env = "MAXIO_ADMIN_PROFILE")]
    pub profile: Option<String>,

    /// Override profile endpoint URL
    #[arg(long, global = true, env = "MAXIO_ADMIN_ENDPOINT")]
    pub endpoint: Option<String>,

    /// Emit JSON instead of human-readable tables
    #[arg(long, global = true)]
    pub json: bool,

    /// Path to config file (default: ~/.config/maxio/config.toml)
    #[arg(long, global = true, env = "MAXIO_ADMIN_CONFIG")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Show config file path and example snippet
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Instance health and readiness (remote)
    Status,

    /// Disk, counts, and active server config (remote)
    Info,

    /// Preflight checks: readiness, disk reserve, keyring (remote or local with --data-dir)
    Doctor {
        /// Offline checks against a data directory (no network)
        #[arg(long, env = "MAXIO_DATA_DIR")]
        data_dir: Option<String>,
    },

    /// Bucket administration (remote)
    #[command(subcommand)]
    Buckets(BucketsCommand),

    /// Trigger stale-multipart / temp-file housekeeping (remote)
    Housekeeping {
        #[command(subcommand)]
        action: HousekeepingAction,
    },

    /// SSE-S3 keyring operations
    #[command(subcommand)]
    Keyring(KeyringCommand),
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Print the default config file path
    Path,
}

#[derive(Subcommand, Debug)]
pub enum BucketsCommand {
    /// List buckets with summary metadata
    List,
    /// Show metadata for one bucket
    Head { name: String },
}

#[derive(Subcommand, Debug)]
pub enum HousekeepingAction {
    /// Run housekeeping once on the target instance
    Run,
}

#[derive(Subcommand, Debug)]
pub enum KeyringCommand {
    /// List key ids and metadata from a running instance (remote)
    List,

    /// Rotate the on-disk keyring (local only — requires filesystem access)
    Rotate {
        /// Data directory containing `.maxio-keys.json`
        #[arg(long, env = "MAXIO_DATA_DIR")]
        data_dir: String,
    },
}

pub struct CommandContext {
    pub json: bool,
    pub profile_name: String,
    pub profile: crate::config::Profile,
}

impl Cli {
    pub async fn run(self) -> Result<()> {
        let Cli {
            profile,
            endpoint,
            json,
            config,
            command,
        } = self;

        match command {
            Command::Config { action } => commands::config::run(action, json),
            Command::Keyring(cmd) => {
                commands::keyring::run(cmd, profile, endpoint, json, config).await
            }
            Command::Status => {
                commands::status::run(build_context(profile, endpoint, json, config).await?).await
            }
            Command::Info => {
                commands::info::run(build_context(profile, endpoint, json, config).await?).await
            }
            Command::Doctor { data_dir } => {
                commands::doctor::run(
                    data_dir,
                    build_context(profile, endpoint, json, config).await?,
                )
                .await
            }
            Command::Buckets(cmd) => {
                commands::buckets::run(cmd, build_context(profile, endpoint, json, config).await?)
                    .await
            }
            Command::Housekeeping { action } => {
                commands::housekeeping::run(
                    action,
                    build_context(profile, endpoint, json, config).await?,
                )
                .await
            }
        }
    }
}

pub async fn build_context(
    profile: Option<String>,
    endpoint: Option<String>,
    json: bool,
    config: Option<PathBuf>,
) -> Result<CommandContext> {
    let file = ConfigFile::load(config.as_deref())?;
    let (profile_name, mut prof) = file.resolve_profile(profile.as_deref())?;
    if let Some(endpoint) = endpoint {
        prof.endpoint = endpoint;
    }
    if prof.endpoint.is_empty() {
        return Err(crate::error::AdminError::Config(format!(
            "profile '{profile_name}' has no endpoint; set endpoint in config or pass --endpoint"
        )));
    }
    Ok(CommandContext {
        json,
        profile_name,
        profile: prof,
    })
}
