use std::fmt::Write as FmtWrite;
use std::io::{self, IsTerminal, Write};
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use env_logger::Builder;
use env_logger::Env;
use log::{debug, error};
use std::fs;
use std::process;
use stow_cm::cli::Cli;
use stow_cm::cli::Commands;
use stow_cm::command::adopt;
use stow_cm::command::clean;
use stow_cm::command::decrypt;
use stow_cm::command::encrypt;
use stow_cm::command::init;
use stow_cm::command::install;
use stow_cm::command::list;
use stow_cm::command::reload;
use stow_cm::command::remove;
use stow_cm::command::resolve_pack_ids;
use stow_cm::command::status;
use stow_cm::config::Config;
use stow_cm::error::Result;
use stow_cm::error::anyhow;
use stow_cm::executor;
use stow_cm::util;

// Avoid musl's default allocator due to lackluster performance
// https://nickb.dev/blog/default-musl-allocator-considered-harmful-to-performance
#[cfg(target_env = "musl")]
use mimalloc::MiMalloc;

#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[allow(clippy::exit)]
fn main() {
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

    let use_color = io::stderr().is_terminal();
    Builder::from_env(Env::default().default_filter_or(default_log_level))
        .format(move |buf, record| {
            let msg = format!("{}", record.args());
            let prefixes = util::get_log_prefixes();
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

    if let Err(e) = run(opt) {
        error!("{e:#}");
        process::exit(1);
    }
}

fn run(opt: Cli) -> Result<()> {
    debug!("opt: {opt:?}");

    let common_config = Arc::new(Some(Config::global()?));
    debug!("common_config: {common_config:?}");

    match opt.command {
        Commands::Install { paths } => {
            let paths = util::canonicalize(paths)?;
            executor::exec_all(&common_config, paths, opt.dry_run, install)?;
        }
        Commands::Remove { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let all_paths = util::canonicalize(all_paths)?;
            executor::exec_all(&common_config, all_paths, opt.dry_run, remove)?;
        }
        Commands::Reload { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let all_paths = util::canonicalize(all_paths)?;
            executor::exec_all(&common_config, all_paths, opt.dry_run, reload)?;
        }
        Commands::Clean { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let paths = util::canonicalize(all_paths)?;
            executor::exec_all(&common_config, paths, opt.dry_run, clean)?;
        }
        Commands::Encrypt { paths } => {
            let paths = util::canonicalize(paths)?;
            executor::exec_all(&common_config, paths, opt.dry_run, encrypt)?;
        }
        Commands::Decrypt { paths } => {
            let paths = util::canonicalize(paths)?;
            executor::exec_all(&common_config, paths, opt.dry_run, decrypt)?;
        }
        Commands::Adopt { sources, to } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow!("global config not loaded"))?;
            let sources = util::canonicalize(sources)?;
            let to = fs::canonicalize(&to).with_context(|| format!("path: {}", to.display()))?;
            for source in &sources {
                adopt(global, source, &to, opt.dry_run)?;
            }
        }
        Commands::List { json } => list(json)?,
        Commands::Status { paths, fix, json } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| anyhow!("global config not loaded"))?;
            status(global, paths, fix, json)?;
        }
        Commands::Init { path, use_defaults } => {
            let global = common_config.as_ref().as_ref();
            init(&path, global, use_defaults)?;
        }
    }

    Ok(())
}
