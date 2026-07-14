use std::sync::Arc;

use clap::Parser;
use env_logger::Env;
use log::debug;

use crate::cli::Cli;
use crate::cli::Commands;
use crate::command::clean;
use crate::command::decrypt;
use crate::command::encrypt;
use crate::command::install;
use crate::command::list;
use crate::command::reload;
use crate::command::remove;
use crate::config::Config;
use crate::error::Result;

mod base64;
mod cli;
mod cli_types;
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
mod paths;
mod symlink;
mod track_file;
mod util;

// Avoid musl's default allocator due to lackluster performance
// https://nickb.dev/blog/default-musl-allocator-considered-harmful-to-performance
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

macro_rules! dispatch {
    ($common_config:expr, $paths:expr, $cmd:ident) => {{
        let paths = util::canonicalize($paths).await?;
        executor::exec_all($common_config, paths, $cmd).await?;
    }};
}

#[tokio::main]
async fn main() -> Result<()> {
    let opt = Cli::parse();

    let default_log_level = if opt.quiet {
        "error"
    } else {
        match opt.verbose {
            0 => "info",
            1 => "debug",
            _ => "trace",
        }
    };

    env_logger::Builder::from_env(Env::default().default_filter_or(default_log_level))
        .default_format()
        .format_level(true)
        .format_target(false)
        .format_module_path(false)
        .format_timestamp(None)
        .init();

    debug!("opt: {opt:?}");

    let common_config = Arc::new(Some(Config::global()?));
    debug!("common_config: {common_config:?}");

    match opt.command {
        Commands::Install { paths } => dispatch!(common_config, paths, install),
        Commands::Remove { paths } => dispatch!(common_config, paths, remove),
        Commands::Reload { paths } => dispatch!(common_config, paths, reload),
        Commands::Clean { paths } => dispatch!(common_config, paths, clean),
        Commands::Encrypt { paths } => dispatch!(common_config, paths, encrypt),
        Commands::Decrypt { paths } => dispatch!(common_config, paths, decrypt),
        Commands::List { json } => list(json).await?,
    }

    Ok(())
}
