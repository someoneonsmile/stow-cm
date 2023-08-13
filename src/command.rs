use anyhow::anyhow;
use futures::prelude::*;
use log::{debug, info, warn};
use regex::RegexSet;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec::Vec;
use tokio::fs;

use crate::config::Config;
use crate::constants::*;
use crate::error::Result;
use crate::merge_tree;
use crate::merge_tree::MergeOption;
use crate::symlink::Symlink;
use crate::util;

/// reload packages
pub(crate) async fn reload(config: Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    remove(config.clone(), &pack).await?;
    install(config, &pack).await?;
    Ok(())
}

/// install packages
pub(crate) async fn install(config: Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = pack
        .file_name()
        .ok_or_else(|| anyhow!("path error: {pack:?}"))?;
    info!("install pack: {pack_name:?}");

    install_link(&config, &pack).await?;

    // execute the init script
    if let Some(command) = &config.init {
        command
            .exec_async(pack.deref(), [(PACK_NAME_ENV, pack_name)])
            .await?;
    }

    Ok(())
}

/// remove packages
async fn install_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack:?} target is none, skip install link");
            return Ok(());
        }
        Some(target) => target.clone(),
    };

    let ignore_re = config
        .ignore
        .as_ref()
        .and_then(|ignore_regexs| RegexSet::new(ignore_regexs).ok());

    let over_re = config
        .over
        .as_ref()
        .and_then(|over_regexs| RegexSet::new(over_regexs).ok());

    let paths = {
        let pack = pack.clone();
        let config = config.clone();
        tokio::task::spawn_blocking(move || {
            let merge_result = merge_tree::MergeTree::new(
                target,
                pack.deref(),
                Some(Arc::new(MergeOption {
                    ignore: ignore_re,
                    over: over_re,
                    fold: config.fold,
                })),
            )
            .merge_add()?;

            if let Some(conflicts) = merge_result.conflicts {
                anyhow::bail!("check conflict: {:?}", conflicts);
            }

            if let Some(expand_symlinks) = merge_result.expand_symlinks {
                // convert symlink dir to dir
                for expand_symlink in expand_symlinks {
                    util::expand_symlink_dir(expand_symlink)?;
                }
            }

            Ok(merge_result.to_create_symlinks.unwrap_or_default()) as Result<Vec<Symlink>>
        })
        .await??
    };

    debug!("{pack:?} install paths: {paths:?}");

    futures::stream::iter(paths.into_iter().map(Ok))
        .try_filter_map(|symlink| async {
            if symlink.dst.exists() {
                // the dir is empty or override regex matched
                if symlink.dst.is_file() || symlink.dst.is_symlink() {
                    fs::remove_file(&symlink.dst).await?;
                } else {
                    fs::remove_dir_all(&symlink.dst).await?;
                }
            }
            let fut = async move {
                info!("install {symlink:?}");
                symlink.create().await
            };
            Ok(Some(fut)) as Result<_>
        })
        .try_for_each_concurrent(None, |future| async move {
            future.await?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// remove packages
pub(crate) async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = pack
        .file_name()
        .ok_or_else(|| anyhow!("path error: {pack:?}"))?;
    info!("remove pack: {pack_name:?}");

    remove_link(&config, &pack).await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        command
            .exec_async(pack.deref(), [(PACK_NAME_ENV, pack_name)])
            .await?;
    }

    Ok(())
}

/// remove packages
async fn remove_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack:?} target is none, skip remove link");
            return Ok(());
        }
        Some(target) => target.clone(),
    };
    let symlinks = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || util::find_prefix_symlink(target, pack.deref()))
            .await??
    };

    debug!("{pack:?} remove paths: {symlinks:?}");

    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_filter_map(|symlink| async {
            let fut = async move {
                info!("remove {symlink:?}");
                symlink.remove().await
            };
            Ok(Some(fut)) as Result<_>
        })
        .try_for_each_concurrent(None, |future| async move {
            future.await?;
            Ok(())
        })
        .await?;

    Ok(())
}
