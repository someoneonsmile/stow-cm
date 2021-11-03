use regex::RegexSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;

use crate::cli::Opt;
use crate::config::{Config, CONFIG_FILE_NAME};
use crate::error::StowResult;
use crate::ignore::CollectBot;
use crate::merge::Merge;

mod cli;
mod config;
mod custom_type;
mod error;
mod ignore;
mod merge;
mod util;

fn main() -> StowResult<()> {
    let opt = Opt::parse();

    let common_config = Config::from_path(format!("./{}", CONFIG_FILE_NAME))?;

    let config = Config::from_cli(&opt)?
        .merge(&common_config)
        .merge(&Some(Default::default()));

    if let Some(to_remove) = opt.to_remove {
        remove_all(&config, to_remove)?;
    }
    if let Some(to_install) = opt.to_install {
        install_all(&config, to_install)?;
    }
    if let Some(to_reload) = opt.to_reload {
        reload_all(&config, to_reload)?;
    }

    Ok(())
}

/// install packages
fn install_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> StowResult<()> {
    for pack in packs {
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        install(&config, fs::canonicalize(&pack)?)?;
    }
    Ok(())
}

/// remove packages
fn remove_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> StowResult<()> {
    for pack in packs {
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        remove(&config, fs::canonicalize(&pack)?)?;
    }
    Ok(())
}

/// remove packages
fn reload_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> StowResult<()> {
    for pack in packs {
        let customer_config = Config::from_path(pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        reload(&config, fs::canonicalize(&pack)?)?;
    }
    Ok(())
}

/// reload packages
fn reload<P: AsRef<Path>>(config: &Config, pack: P) -> StowResult<()> {
    remove(config, fs::canonicalize(&pack)?)?;
    install(config, fs::canonicalize(&pack)?)?;
    Ok(())
}

/// install packages
fn install<P: AsRef<Path>>(config: &Config, pack: P) -> StowResult<()> {
    println!("install pack: {:?}", pack.as_ref());
    let target = config.target.as_ref().ok_or("target is None")?;
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
fn remove<P: AsRef<Path>>(config: &Config, pack: P) -> StowResult<()> {
    println!("remove pack: {:?}", pack.as_ref());
    let target = config.target.as_ref().ok_or("config target is None")?;
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
