use clap::Parser;
use env_logger::Env;
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
use crate::error::Result;

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
    env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .default_format()
        .format_level(true)
        .format_target(false)
        .format_module_path(false)
        .format_timestamp(None)
        .init();

    let opt = Cli::parse();
    debug!("opt: {:?}", opt);

    let common_config = Arc::new(Some(Config::global()?));
    debug!("common_config: {common_config:?}");

    match opt.command {
        Commands::Install { paths } => {
            // let common_config = common_config.clone();
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, install).await?;
        }
        Commands::Remove { paths } => {
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, remove).await?;
        }
        Commands::Reload { paths } => {
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, reload).await?;
        }
        Commands::Clean { paths } => {
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, clean).await?;
        }
        Commands::Encrypt { paths } => {
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, encrypt).await?;
        }
        Commands::Decrypt { paths } => {
            let paths = util::canonicalize(paths).await?;
            executor::exec_all(common_config, paths, decrypt).await?;
        }
    };

    Ok(())
}
