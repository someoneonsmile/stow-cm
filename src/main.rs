use regex::RegexSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use tokio::task::JoinHandle;

use crate::cli::Opt;
use crate::config::{Config, CONFIG_FILE_NAME};
use crate::error::{anyhow, Result};
use crate::ignore::CollectBot;
use crate::merge::Merge;

mod cli;
mod config;
mod custom_type;
mod error;
mod ignore;
mod merge;
mod util;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_default_env()
        .parse_filters("info")
        .default_format()
        .format_level(false)
        .format_target(false)
        .format_module_path(false)
        .format_timestamp(None)
        .init();

    let opt = Opt::parse();

    let common_config = Config::from_path(format!("./{}", CONFIG_FILE_NAME))?;

    let config = Config::from_cli(&opt)?
        .merge(&common_config)
        .merge(&Some(Default::default()));

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
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        handles.push(tokio::spawn(async move {
            install(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }))
    }
    for handle in handles {
        let _ = handle.await?;
    }
    Ok(())
}

/// remove packages
async fn remove_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        handles.push(tokio::spawn(async move {
            remove(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }))
    }
    for handle in handles {
        let _ = handle.await?;
    }
    Ok(())
}

/// remove packages
async fn reload_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        handles.push(tokio::spawn(async move {
            reload(&config, fs::canonicalize(&pack)?).await?;
            Ok(())
        }));
    }
    for handle in handles {
        let _ = handle.await?;
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
    log::info!("install pack: {:?}", pack.as_ref());
    let target = config.target.as_ref().ok_or(anyhow!("target is None"))?;
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
        let entry_target = PathBuf::from(&target).join(path.strip_prefix(pack.as_ref())?);
        if entry_target.exists() {
            if let Some(true) = &config.force {
            } else {
                if let Ok(false) = same_file::is_same_file(&path, &entry_target) {
                    log::error!("target has exists, target:{:?}", entry_target);
                }
                continue;
            }
        }
        handles.push(tokio::spawn(async move {
            if let Some(parent) = entry_target.parent() {
                fs::create_dir_all(parent)?;
            }
            let _ = fs::remove_file(&entry_target);
            log::info!("install {:?} -> {:?}", entry_target, path);
            symlink(&path, &entry_target)?;
            Ok(())
        }));
    }
    for handle in handles {
        let _ = handle.await?;
    }

    Ok(())
}

/// remove packages
async fn remove<P: AsRef<Path>>(config: &Config, pack: P) -> Result<()> {
    log::info!("remove pack: {:?}", pack.as_ref());
    let target = config
        .target
        .as_ref()
        .ok_or(anyhow!("config target is None"))?;
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
        let entry_target =
            PathBuf::from(&target).join(PathBuf::from(&path).strip_prefix(pack.as_ref())?);
        if !entry_target.exists() {
            let _ = fs::remove_file(&entry_target);
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&entry_target, &path) {
            log::error!("remove symlink, not same_file, target:{:?}", entry_target);
            continue;
        }
        handles.push(tokio::spawn(async move {
            log::info!("remove {:?} -> {:?}", entry_target, path);
            fs::remove_file(&entry_target)?;
            Ok(())
        }))
    }
    for handle in handles {
        let _ = handle.await?;
    }

    Ok(())
}
