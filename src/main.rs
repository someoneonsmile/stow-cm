use clap::Parser;
use log::debug;

use std::sync::Arc;

use crate::cli::Cli;
use crate::cli::Commands;
use crate::command::clean;
use crate::command::decrypt;
use crate::command::encrypt;
use crate::command::install;
use crate::command::reload;
use crate::command::remove;
use crate::config::Config;
use crate::constants::*;
use crate::error::Result;
use crate::merge::Merge;

mod base64;
mod cli;
mod command;
mod config;
mod constants;
mod crypto;
mod custom_type;
mod dev;
mod error;
mod executor;
mod merge;
mod merge_tree;
mod symlink;
mod track_file;
mod util;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .parse_filters("debug")
        .default_format()
        .format_level(true)
        .format_target(false)
        .format_module_path(false)
        .format_timestamp(None)
        .init();

    let opt = Cli::parse();
    debug!("opt: {:?}", opt);

    let common_config = Config::from_path(GLOBAL_CONFIG_FILE)?;
    let common_config = common_config.merge(Some(Default::default()));
    let common_config = Arc::new(common_config);

    debug!("common_config: {common_config:?}");

    match opt.command {
        Commands::Install { paths } => {
            // let common_config = common_config.clone();
            executor::exec_all(common_config, paths, install).await?;
        }
        Commands::Remove { paths } => {
            executor::exec_all(common_config, paths, remove).await?;
        }
        Commands::Reload { paths } => {
            executor::exec_all(common_config, paths, reload).await?;
        }
        Commands::Clean { paths } => {
            executor::exec_all(common_config, paths, clean).await?;
        }
        Commands::Encrypt { paths } => {
            executor::exec_all(common_config, paths, encrypt).await?;
        }
        Commands::Decrypt { paths } => {
            executor::exec_all(common_config, paths, decrypt).await?;
        }
    };

    Ok(())
}
