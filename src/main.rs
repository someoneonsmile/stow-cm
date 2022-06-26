use anyhow::Context;
use futures::prelude::*;
use log::{debug, info, warn};
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
use crate::config::{Config, CONFIG_FILE_NAME};
use crate::error::{anyhow, Result};

mod cli;
mod config;
mod custom_type;
mod error;
mod merge;
mod merge_tree;
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
                    "{:?} is not the pack_home (which contains .stowrc config file)",
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
    let ignore_re = match config.ignore.as_ref() {
        Some(ignore_regexs) => RegexSet::new(ignore_regexs).ok(),
        None => None,
    };

    let paths = {
        let pack = pack.clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let target = config
                .target
                .as_ref()
                .ok_or_else(|| anyhow!("target is None"))?;
            let merge_result =
                merge_tree::MergeTree::new(target, pack.deref(), &ignore_re).merge_add()?;

            if let Some(conflicts) = merge_result.conflicts {
                anyhow::bail!("check conflict: {:?}", conflicts);
            }

            if let Some(expand_symlinks) = merge_result.expand_symlinks {
                // convert symlink dir to dir
                for expand_symlink in expand_symlinks {
                    util::expand_symlink_dir(expand_symlink)?;
                }
            }

            Ok(merge_result.install_paths.unwrap_or_default()) as Result<Vec<(PathBuf, PathBuf)>>
        })
        .await??
    };

    debug!("{pack:?} install paths: {paths:?}");

    futures::stream::iter(paths.into_iter().map(Ok))
        .try_filter_map(|(path_source, path_target)| async {
            if path_target.exists() {
                // remove the empty dir
                if util::is_empty_dir(&path_target) {
                    fs::remove_dir_all(&path_target).await?;
                } else {
                    anyhow::bail!("install conflict: {path_target:?}");
                }
            }
            let fut = async move {
                if let Some(parent) = path_target.parent() {
                    fs::create_dir_all(parent).await?;
                }
                info!("install {:?} -> {:?}", path_target, path_source);
                fs::symlink(&path_source, &path_target).await?;

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

    let paths = {
        let pack = pack.clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let target = config
                .target
                .as_ref()
                .ok_or_else(|| anyhow!("config target is None"))?;
            util::find_symlink_at(target, pack.deref())
        })
        .await??
    };

    debug!("{pack:?} remove paths: {paths:?}");

    futures::stream::iter(paths.into_iter().map(Ok))
        .try_filter_map(|(path_link, path_source)| async {
            let fut = async move {
                info!("remove {:?} -> {:?}", path_link, path_source);
                fs::remove_file(&path_link)
                    .await
                    .with_context(|| format!("path: {path_link:?}"))?;
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
