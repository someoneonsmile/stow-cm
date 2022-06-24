use futures::prelude::*;
use log::{debug, error, info, warn};
use merge::MergeLazy;
use regex::RegexSet;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec::Vec;
use tokio::fs;
use tokio::task::JoinHandle;

use crate::cli::Opt;
use crate::collect_bot::CollectBot;
use crate::config::{Config, CONFIG_FILE_NAME};
use crate::error::{anyhow, Result};

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

    let common_config = Config::from_path("${XDG_CONFIG_HOME:-~/.config}/stow/config")?;
    let common_config = Arc::new(common_config);

    debug!("common_config: {:?}", common_config);

    if let Some(to_remove) = opt.to_remove {
        let common_config = common_config.clone();
        exec_all(common_config, to_remove, remove).await?;
    }
    if let Some(to_install) = opt.to_install {
        let common_config = common_config.clone();
        exec_all(common_config, to_install, install).await?;
    }
    if let Some(to_reload) = opt.to_reload {
        let common_config = common_config.clone();
        exec_all(common_config, to_reload, reload).await?;
    }

    Ok(())
}

/// exec packages
async fn exec_all<F, P, Fut>(common_config: Arc<Option<Config>>, packs: Vec<P>, f: F) -> Result<()>
where
    F: Fn(Arc<Config>, P) -> Fut,
    P: AsRef<Path>,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    futures::stream::iter(packs.into_iter().map(Ok))
        .try_filter_map(|pack| async {
            let pack_config = Config::from_path(pack.as_ref().join(CONFIG_FILE_NAME))?;
            // TODO: maybe the pack_config can be optional
            if pack_config.is_none() {
                warn!(
                    "{:?} is not the pack_home (witch contains .stowrc config file)",
                    pack.as_ref()
                );
                return Ok(None);
            };
            let config = match pack_config.merge_lazy(|| common_config.deref().clone()) {
                Some(v) => Arc::new(v),
                None => unreachable!(),
            };
            let fut = tokio::spawn((f)(config, pack));
            Ok(Some(fut)) as Result<Option<JoinHandle<Result<()>>>>
        })
        .try_for_each_concurrent(None, |future| async move {
            future.await??;
            Ok(())
        })
        .await?;

    Ok(())
}

/// reload packages
async fn reload<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    remove(config.clone(), &pack).await?;
    install(config, &pack).await?;
    Ok(())
}

/// install packages
async fn install<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    info!("install pack: {:?}", pack);
    let target = config
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let paths = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || {
            let mut paths = Vec::new();
            for entry in std::fs::read_dir(pack.deref())? {
                let (_, sub_path_option) = CollectBot::new(entry?.path(), &ignore_re).collect()?;
                if let Some(mut sub_paths) = sub_path_option {
                    paths.append(&mut sub_paths);
                }
            }
            Ok(paths) as Result<Vec<PathBuf>>
        })
        .await??
    };

    futures::stream::iter(paths.into_iter().map(Ok))
        .try_filter_map(|path| async {
            let path_target = PathBuf::from(target).join(path.strip_prefix(pack.deref())?);
            if path_target.exists() {
                if let Some(false) | None = config.force {
                    if let Ok(false) | Err(_) = same_file::is_same_file(&path, &path_target) {
                        // TODO: return error
                        error!(
                            "target has exists and not same file, target:{:?}",
                            path_target
                        );
                        return Ok(None);
                    }
                }
            }
            let fut = async move {
                if path_target.exists() {
                    fs::remove_file(&path_target).await?;
                }
                if let Some(parent) = path_target.parent() {
                    fs::create_dir_all(parent).await?;
                }
                info!("install {:?} -> {:?}", path_target, path);
                fs::symlink(&path, &path_target).await?;

                Ok(()) as Result<()>
            };
            Ok(Some(fut)) as Result<_>
        })
        .try_for_each_concurrent(None, |future| async move {
            future.await?;
            Ok(())
        })
        .await?;

    // execute the init script
    if let Some(command) = &config.init {
        command.exec_async(pack.deref()).await?;
    }

    Ok(())
}

/// remove packages
async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    info!("remove pack: {:?}", pack);
    let target = config
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("config target is None"))?;
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let paths = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || {
            let mut paths = Vec::new();
            for entry in std::fs::read_dir(pack.deref())? {
                let (_, sub_path_option) = CollectBot::new(entry?.path(), &ignore_re).collect()?;
                if let Some(mut sub_paths) = sub_path_option {
                    paths.append(&mut sub_paths);
                }
            }
            Ok(paths) as Result<Vec<PathBuf>>
        })
        .await??
    };

    futures::stream::iter(paths.into_iter().map(Ok))
        .try_filter_map(|path| async {
            let path_target = PathBuf::from(target).join(path.strip_prefix(pack.deref())?);
            if !path_target.exists() {
                return Ok(None);
            }
            if !same_file::is_same_file(&path, &path_target)? {
                error!("remove symlink, not same_file, target:{:?}", path_target);
                return Ok(None);
            }
            let fut = async move {
                info!("remove {:?} -> {:?}", path_target, path);
                fs::remove_file(&path_target).await?;
                Ok(()) as Result<()>
            };
            Ok(Some(fut)) as Result<_>
        })
        .try_for_each_concurrent(None, |future| async move {
            future.await?;
            Ok(())
        })
        .await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        command.exec_async(pack.deref()).await?;
    }

    Ok(())
}
