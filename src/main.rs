use std::fmt::Write as FmtWrite;
use std::io::{IsTerminal, Write};
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use env_logger::Env;
use log::{debug, error};
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

// Avoid musl's default allocator due to lackluster performance
// https://nickb.dev/blog/default-musl-allocator-considered-harmful-to-performance
#[cfg(target_env = "musl")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

macro_rules! dispatch {
    ($common_config:expr, $paths:expr, $cmd:ident) => {{
        let paths = stow_cm::util::canonicalize($paths)?;
        stow_cm::executor::exec_all(&$common_config, paths, $cmd)?;
    }};
}

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

    let use_color = std::io::stderr().is_terminal();
    env_logger::Builder::from_env(Env::default().default_filter_or(default_log_level))
        .format(move |buf, record| {
            let msg = format!("{}", record.args());
            let prefixes = stow_cm::util::get_log_prefixes();
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
        std::process::exit(1);
    }
}

fn run(opt: Cli) -> Result<()> {
    debug!("opt: {opt:?}");

    let common_config = Arc::new(Some(Config::global()?));
    debug!("common_config: {common_config:?}");

    match opt.command {
        Commands::Install { paths } => {
            let paths = stow_cm::util::canonicalize(paths)?;
            let dry_run = opt.dry_run;
            stow_cm::executor::exec_all(&common_config, paths, |config, pack| {
                install(config, pack, dry_run)
            })?;
        }
        Commands::Remove { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let all_paths = stow_cm::util::canonicalize(all_paths)?;
            stow_cm::executor::exec_all(&common_config, all_paths, |config, pack| {
                remove(config, pack, opt.dry_run)
            })?;
        }
        Commands::Reload { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let all_paths = stow_cm::util::canonicalize(all_paths)?;
            stow_cm::executor::exec_all(&common_config, all_paths, |config, pack| {
                reload(config, pack, opt.dry_run)
            })?;
        }
        Commands::Clean { paths, ids } => {
            let mut all_paths = paths;
            if !ids.is_empty() {
                all_paths.extend(resolve_pack_ids(&ids)?);
            }
            let dry_run = opt.dry_run;
            let paths = stow_cm::util::canonicalize(all_paths)?;
            stow_cm::executor::exec_all(&common_config, paths, |config, pack| {
                clean(config, pack, dry_run)
            })?;
        }
        Commands::Encrypt { paths } => dispatch!(common_config, paths, encrypt),
        Commands::Decrypt { paths } => dispatch!(common_config, paths, decrypt),
        Commands::Adopt { sources, to } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| stow_cm::error::anyhow!("global config not loaded"))?;
            let sources = stow_cm::util::canonicalize(sources)?;
            let to =
                std::fs::canonicalize(&to).with_context(|| format!("path: {}", to.display()))?;
            for source in &sources {
                adopt(global, source, &to, opt.dry_run)?;
            }
        }
        Commands::List { json } => list(json)?,
        Commands::Status { paths, fix, json } => {
            let global = common_config
                .as_ref()
                .as_ref()
                .ok_or_else(|| stow_cm::error::anyhow!("global config not loaded"))?;
            status(global, paths, fix, json)?;
        }
        Commands::Init { path, use_defaults } => {
            let global = common_config.as_ref().as_ref();
            init(&path, global, use_defaults)?;
        }
    }

    Ok(())
}
