use std::fmt::Write as FmtWrite;
use std::io::{IsTerminal, Write};
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
use crate::command::init;
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
        let paths = util::canonicalize($paths)?;
        executor::exec_all(&$common_config, paths, $cmd)?;
    }};
}

fn main() -> Result<()> {
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

    let use_color = std::io::stderr().is_terminal();
    env_logger::Builder::from_env(Env::default().default_filter_or(default_log_level))
        .format(move |buf, record| {
            let msg = format!("{}", record.args());
            let prefixes = crate::util::get_log_prefixes();
            let styled = if prefixes.is_empty() {
                msg
            } else if use_color {
                let mut parts = String::new();
                for (name, color_idx) in &prefixes {
                    let c = match color_idx % 6 {
                        0 => "\x1b[1;36m", // bold cyan
                        1 => "\x1b[1;33m", // bold yellow
                        2 => "\x1b[1;35m", // bold magenta
                        3 => "\x1b[1;32m", // bold green
                        4 => "\x1b[1;34m", // bold blue
                        _ => "\x1b[1;37m", // bold white
                    };
                    let _ = write!(parts, "{c}{name}\x1b[0m: ");
                }
                parts.push_str(&msg);
                parts
            } else {
                let prefix_str: Vec<&str> = prefixes.iter().map(|(n, _)| n.as_str()).collect();
                format!("{}: {msg}", prefix_str.join(": "))
            };
            let level = record.level();
            let level_style = buf.default_level_style(level);
            writeln!(buf, "{level_style}[{level}]{level_style:#}  {styled}")
        })
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
            let sources = util::canonicalize(sources)?;
            let to = match std::fs::canonicalize(&to) {
                Ok(resolved) => resolved,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => to,
                Err(e) => return Err(e).with_context(|| format!("path: {}", to.display())),
            };
            adopt(global, &sources, &to)?;
        }
        Commands::List { json } => list(json)?,
        Commands::Status { paths, fix, json } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| crate::error::anyhow!("global config not loaded"))?;
            status(global, paths, fix, json)?;
        }
        Commands::Init { path, use_defaults } => {
            let global = common_config.as_ref().as_ref();
            init(&path, global, use_defaults)?;
        }
    }

    Ok(())
}
