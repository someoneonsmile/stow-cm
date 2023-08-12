use futures::prelude::*;
use log::{debug, error, info, warn};
use merge::MergeWith;
use regex::RegexSet;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec::Vec;
use tokio::fs;
use tokio::task::JoinHandle;

use crate::cli::Opt;
use crate::config::Config;
use crate::constants::*;
use crate::error::Result;
use crate::merge::MergeDefault;
use crate::merge_tree::MergeOption;
use crate::symlink::Symlink;

mod cli;
mod config;
mod constants;
mod custom_type;
mod error;
mod merge;
mod merge_tree;
mod symlink;
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

    let common_config = Config::from_path("${XDG_CONFIG_HOME:-~/.config}/stow-cm/config.toml")?;
    let common_config = common_config.merge_default();
    let common_config = Arc::new(common_config);

    debug!("common_config: {common_config:?}");

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
                error!(
                    "{:?} is not the pack_home (which contains {} config file)",
                    pack.as_ref(),
                    CONFIG_FILE_NAME
                );
                return Ok(None);
            };
            let pack_name = pack
                .as_ref()
                .file_name()
                .and_then(|it| it.to_str().map(|it| it.to_owned()))
                .ok_or_else(|| anyhow::anyhow!("path error"))?;
            let mut config = match pack_config.merge_with(|| common_config.deref().clone()) {
                Some(config) => config,
                None => unreachable!("no config"),
            };

            let context_map: HashMap<&str, String, RandomState> =
                HashMap::from_iter(vec![(PACK_NAME_ENV, pack_name)]);
            config.target = match config.target.as_mut() {
                Some(target) => Some(util::shell_expend_full_with_context(target, |key| {
                    context_map.get(key)
                })?),
                None => None,
            };
            let fut = tokio::spawn((f)(Arc::new(config), pack));
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
async fn reload(config: Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    remove(config.clone(), &pack).await?;
    install(config, &pack).await?;
    Ok(())
}

/// install packages
async fn install(config: Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = match pack.file_name() {
        Some(pack_name) => pack_name,
        None => unreachable!(),
    };
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
async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = match pack.file_name() {
        Some(pack_name) => pack_name,
        None => unreachable!(),
    };
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
