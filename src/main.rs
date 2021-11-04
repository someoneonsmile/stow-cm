use anyhow::{anyhow, Result};
use regex::RegexSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use tokio::task::JoinHandle;
use std::sync::Arc;

use crate::cli::Opt;
use crate::config::{Config, CONFIG_FILE_NAME};
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
    let opt = Opt::parse();

    let common_config = Config::from_path(format!("./{}", CONFIG_FILE_NAME))?;

    let config = Config::from_cli(&opt)?
        .merge(&common_config)
        .merge(&Some(Default::default()));

    let config = Arc::new(config);

    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    if let Some(to_remove) = opt.to_remove {
        let config = config.clone();
        handles.push(tokio::spawn(async move {
            remove_all(config, to_remove).await?;
            Ok(())
        }));
    }
    if let Some(to_install) = opt.to_install {
        let config = config.clone();
        handles.push(tokio::spawn(async move {
            install_all(config, to_install).await?;
            Ok(())
        }));
    }
    if let Some(to_reload) = opt.to_reload {
        let config = config.clone();
        handles.push(tokio::spawn(async move {
            reload_all(config, to_reload).await?;
            Ok(())
        }));
    }
    for handle in handles {
        let _ = handle.await?;
    }

    Ok(())
}

/// install packages
async fn install_all(common_config: Arc<Option<Config>>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let common_config = common_config.clone();
        handles.push(tokio::spawn(async move {
            let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
            let config = customer_config.merge(&common_config).unwrap_or_default();
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
async fn remove_all(common_config: Arc<Option<Config>>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let common_config = common_config.clone();
        handles.push(tokio::spawn(async move {
            let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
            let config = customer_config.merge(&common_config).unwrap_or_default();
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
async fn reload_all(common_config: Arc<Option<Config>>, packs: Vec<PathBuf>) -> Result<()> {
    let mut handles = Vec::<JoinHandle<Result<()>>>::new();
    for pack in packs {
        let common_config = common_config.clone();
        handles.push(tokio::spawn(async move {
            let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
            let config = customer_config.merge(&common_config).unwrap_or_default();
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
    println!("install pack: {:?}", pack.as_ref());
    let target = config.target.as_ref().ok_or(anyhow!("target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let mut paths = Vec::new();
    for path in fs::read_dir(pack.as_ref())? {
        let (_, sub_path_option) = CollectBot::new(path?.path(), &ignore_re).collect()?;
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }

    for path in paths {
        let entry_target = PathBuf::from(&target).join(path.strip_prefix(pack.as_ref())?);
        if entry_target.exists() {
            if let Some(true) = &config.force {
            } else {
                if let Ok(false) = same_file::is_same_file(&path, &entry_target) {
                    eprintln!("target has exists, target:{:?}", entry_target);
                }
                continue;
            }
        }
        if let Some(parent) = entry_target.parent() {
            fs::create_dir_all(parent)?;
        }
        let _ = fs::remove_file(&entry_target);
        println!("install {:?} -> {:?}", entry_target, path);
        symlink(&path, &entry_target)?;
    }

    Ok(())
}

/// remove packages
async fn remove<P: AsRef<Path>>(config: &Config, pack: P) -> Result<()> {
    println!("remove pack: {:?}", pack.as_ref());
    let target = config.target.as_ref().ok_or(anyhow!("config target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let mut paths = Vec::new();
    for path in fs::read_dir(&pack).unwrap() {
        let (_, sub_path_option) = CollectBot::new(&path?.path(), &ignore_re).collect()?;
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }
    for path in paths {
        let entry_target =
            PathBuf::from(&target).join(PathBuf::from(&path).strip_prefix(pack.as_ref())?);
        if !entry_target.exists() {
            let _ = fs::remove_file(&entry_target);
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&entry_target, &path) {
            eprintln!("remove symlink, not same_file, target:{:?}", entry_target);
            continue;
        }
        println!("remove {:?} -> {:?}", entry_target, path);
        fs::remove_file(&entry_target)?;
    }
    Ok(())
}
