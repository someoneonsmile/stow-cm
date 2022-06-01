use log::{debug, error, info, warn};
use regex::RegexSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use tokio::task::JoinHandle;

use crate::cli::Opt;
use crate::collect_bot::CollectBot;
use crate::config::{Config, CONFIG_FILE_NAME};
use crate::error::{anyhow, Result};
use crate::merge::Merge;

mod cli;
mod collect_bot;
mod config;
mod custom_type;
mod error;
mod merge;
mod util;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .parse_filters("info")
        .default_format()
        .format_level(true)
        .format_target(false)
        .format_module_path(false)
        .format_timestamp(None)
        .init();

    let opt = Opt::parse();
    debug!("opt: {:?}", opt);

    // TODO: path config fixed (ex: ~/.config/stow/config)
    // TODO: make the cli config not be override ?
    let common_config = Config::from_path("$XDG_CONFIG_HOME/stow/config")?;
    let common_config =
        common_config.merge(Config::from_path(format!("./{:?}", CONFIG_FILE_NAME))?);
    debug!("common_config: {:?}", common_config);

    let config = Config::from_cli(&opt)?
        .merge(common_config)
        .merge(Some(Default::default()));

    if let Some(to_remove) = opt.to_remove {
        remove_all(&config, to_remove).await?;
    }
    if let Some(to_install) = opt.to_install {
        install_all(&config, to_install).await?;
    }
    if let Some(to_reload) = opt.to_reload {
        reload_all(&config, to_reload).await?;
    }

    Ok(())
}

/// install packages
async fn install_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        // TODO: rename to home_config or pack_config, and determine whether it is a pack
        // should we move the resolve pack config and judge if it's a valid pack logic into install ?
        let pack_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        if pack_config.is_none() {
            warn!(
                "{:?} is not the pack_home (witch contains .stowrc config file)",
                pack
            );
            continue;
        }
        let config = pack_config.merge(common_config.clone()).unwrap();
        handles.push(tokio::spawn(async move {
            install(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }))
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

/// remove packages
async fn remove_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let pack_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        if pack_config.is_none() {
            warn!(
                "{:?} is not the pack_home (witch contains .stowrc config file)",
                pack
            );
            continue;
        }
        let config = pack_config.merge(common_config.clone()).unwrap();
        handles.push(tokio::spawn(async move {
            remove(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }))
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

/// remove packages
async fn reload_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let pack_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        if pack_config.is_none() {
            warn!(
                "{:?} is not the pack_home (witch contains .stowrc config file)",
                pack
            );
            continue;
        }
        let config = pack_config.merge(common_config.clone()).unwrap();
        handles.push(tokio::spawn(async move {
            reload(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }));
    }
    // TODO: replace with await all, and handle the result
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

/// reload packages
async fn reload<P: AsRef<Path>>(config: &Config, pack: P) -> Result<()> {
    remove(config, fs::canonicalize(&pack)?).await?;
    install(config, fs::canonicalize(&pack)?).await?;
    Ok(())
}

/// install packages
async fn install<P: AsRef<Path>>(config: &Config, pack: P) -> Result<()> {
    info!("install pack: {:?}", pack.as_ref());
    let target = config
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let mut paths = Vec::new();
    for entry in fs::read_dir(pack.as_ref())? {
        let (_, sub_path_option) = CollectBot::new(entry?.path(), &ignore_re).collect()?;
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }

    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for path in paths {
        let path_target = PathBuf::from(&target).join(path.strip_prefix(pack.as_ref())?);
        if path_target.exists() {
            if let Some(true) = &config.force {
            } else {
                if let Ok(false) = same_file::is_same_file(&path, &path_target) {
                    error!("target has exists, target:{:?}", path_target);
                }
                continue;
            }
        }
        handles.push(tokio::task::spawn_blocking(move || {
            if let Some(parent) = path_target.parent() {
                fs::create_dir_all(parent)?;
            }
            let _ = fs::remove_file(&path_target);
            info!("install {:?} -> {:?}", path_target, path);
            symlink(&path, &path_target)?;
            Ok(())
        }));
    }
    for handle in handles {
        handle.await??;
    }

    Ok(())
}

/// remove packages
async fn remove<P: AsRef<Path>>(config: &Config, pack: P) -> Result<()> {
    info!("remove pack: {:?}", pack.as_ref());
    let target = config
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("config target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let mut paths = Vec::new();
    for entry in fs::read_dir(&pack)? {
        let (_, sub_path_option) = CollectBot::new(&entry?.path(), &ignore_re).collect()?;
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }

    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for path in paths {
        let path_target =
            PathBuf::from(&target).join(PathBuf::from(&path).strip_prefix(pack.as_ref())?);
        if !path_target.exists() {
            let _ = fs::remove_file(&path_target);
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&path_target, &path) {
            error!("remove symlink, not same_file, target:{:?}", path_target);
            continue;
        }
        handles.push(tokio::task::spawn_blocking(move || {
            info!("remove {:?} -> {:?}", path_target, path);
            fs::remove_file(&path_target)?;
            Ok(())
        }))
    }
    // TODO: replace with await all, and handle the result
    for handle in handles {
        handle.await??;
    }

    Ok(())
}
