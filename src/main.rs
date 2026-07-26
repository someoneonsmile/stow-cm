use std::ffi::OsStr;
use std::fmt::Write as FmtWrite;
use std::io::{IsTerminal, Write};
use std::sync::Arc;

use anyhow::Context;
use clap::Parser;
use env_logger::Env;
use log::debug;
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
            let to = match std::fs::canonicalize(&to) {
                Ok(resolved) => resolved,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => to,
                Err(e) => return Err(e).with_context(|| format!("path: {}", to.display())),
            };
            let config_file = stow_cm::constants::CONFIG_FILE_NAME;
            let mut pack_dirs = Vec::new();
            for source in &sources {
                let pack_name = source
                    .file_name()
                    .and_then(OsStr::to_str)
                    .ok_or_else(|| {
                        stow_cm::error::anyhow!("{}: cannot determine pack name", source.display())
                    })?;
                let pack_dir = to.join(pack_name);
                let config_path = pack_dir.join(config_file);

                if pack_dir.exists() && !config_path.exists() {
                    let mut entries = std::fs::read_dir(&pack_dir)?;
                    if entries.next().is_some() {
                        return Err(stow_cm::error::anyhow!(
                            "pack directory '{}' already exists with content \
                             but no {config_file} — refusing to adopt.\n\
                             Remove the directory or create a {config_file} first.",
                            pack_dir.display()
                        ));
                    }
                }
                if !opt.dry_run {
                    std::fs::create_dir_all(&pack_dir)?;
                }
                if !config_path.exists() {
                    if opt.dry_run {
                        debug!("would generate stow-cm.toml for {pack_name} (dry-run)");
                    } else {
                        let content = format!(
                            "# Auto-generated by stow-cm adopt\n\
                             name = \"{pack_name}\"\n\
                             # target inherits from global config (default: ${{XDG_CONFIG_HOME:-~/.config}}/${{PACK_NAME}}/);\n\
                             # for this pack it resolves to: \"{}\"\n",
                            source.display()
                        );
                        std::fs::write(&config_path, content).map_err(|e| {
                            stow_cm::error::anyhow!(
                                "failed to write {}: {e}",
                                config_path.display()
                            )
                        })?;
                        debug!("generated stow-cm.toml for {pack_name}");
                    }
                }
                let pack_config = Config::for_pack(&pack_dir, global, None, false)?;
                let target = pack_config
                    .target
                    .as_ref()
                    .ok_or_else(|| stow_cm::error::anyhow!("target is not configured"))?;
                let tc = std::fs::canonicalize(target);
                let sc = std::fs::canonicalize(source);
                let target_matches = match (tc, sc) {
                    (Ok(tc), Ok(sc)) => tc == sc,
                    _ => false,
                };
                if !target_matches {
                    return Err(stow_cm::error::anyhow!(
                        "target in stow-cm.toml does not match source '{}'",
                        source.display()
                    ));
                }
                pack_dirs.push(pack_dir);
            }
            let dry_run = opt.dry_run;
            stow_cm::executor::exec_all(&common_config, pack_dirs, |config, pack| {
                adopt(config, pack, dry_run)
            })?;
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
