use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use env_logger::Env;
use log::debug;

use crate::cli::Cli;
use crate::cli::Commands;
use crate::command::adopt;
use crate::command::clean;
use crate::command::decrypt;
use crate::command::encrypt;
use crate::command::install;
use crate::command::list;
use crate::command::reload;
use crate::command::remove;
use crate::command::resolve_pack_ids;
use crate::command::status;
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
        Commands::Remove { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            dispatch!(common_config, all_paths, remove);
        }
        Commands::Reload { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            dispatch!(common_config, all_paths, reload);
        }
        Commands::Clean { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            dispatch!(common_config, all_paths, clean);
        }
        Commands::Encrypt { paths } => dispatch!(common_config, paths, encrypt),
        Commands::Decrypt { paths } => dispatch!(common_config, paths, decrypt),
        Commands::Adopt { sources, to } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| crate::error::anyhow!("global config not loaded"))?;
            let sources = util::canonicalize(sources).await?;
            let to = if tokio::fs::try_exists(&to).await.unwrap_or(false) {
                tokio::fs::canonicalize(&to)
                    .await
                    .with_context(|| format!("path: {}", to.display()))?
            } else {
                to
            };
            adopt(global, sources, &to).await?;
        }
        Commands::List { json } => list(json).await?,
        Commands::Status { paths, fix, json } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| crate::error::anyhow!("global config not loaded"))?;
            status(global, paths, fix, json).await?;
        }
    }

    Ok(())
}
