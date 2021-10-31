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
mod error;
mod ignore;
mod merge;
mod util;

fn main() -> StowResult<()> {
    let opt = Opt::parse();
    // println!("{:?}", opt);
    // println!("{:#?}", opt);

    let common_config =
        Config::from_path(format!("./{}", CONFIG_FILE_NAME))?.merge(&Some(Default::default()));

    remove_all(&common_config, opt.to_remove)?;
    install_all(&common_config, opt.to_install)?;

    Ok(())
}

/// install packages
fn install_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> StowResult<()> {
    for pack in packs {
        let customer_config = Config::from_path(&pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        install(config, pack)?;
    }
    Ok(())
}

/// install packages
fn install<P: AsRef<Path>>(config: Config, pack: P) -> StowResult<()> {
    println!("install pack: {:?}", pack.as_ref());
    let target = config.target.ok_or("target is None")?;
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
            if let Ok(false) = same_file::is_same_file(&path, &entry_target) {
                eprintln!("target has exists, target:{:?}", entry_target);
            }
            continue;
        }
        if let Some(parent) = entry_target.parent() {
            fs::create_dir_all(parent)?;
        }
        println!("{:?} -> {:?}", entry_target, path);
        symlink(&path, &entry_target)?;
    }

    Ok(())
}

/// remove packages
fn remove_all(common_config: &Option<Config>, packs: Vec<PathBuf>) -> StowResult<()> {
    for pack in packs {
        let customer_config = Config::from_path(&pack.join(CONFIG_FILE_NAME))?;
        let config = customer_config.merge(common_config).unwrap_or_default();
        remove(config, pack)?;
    }
    Ok(())
}

/// remove packages
fn remove<P: AsRef<Path>>(config: Config, pack: P) -> StowResult<()> {
    println!("remove pack: {:?}", pack.as_ref());
    let target = config.target.ok_or("config target is None")?;
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
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&entry_target, &path) {
            eprintln!("remove symlink, not same_file, target:{:?}", entry_target);
            continue;
        }
        fs::remove_file(&entry_target)?;
    }
    Ok(())
}
