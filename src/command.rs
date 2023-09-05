use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use futures::prelude::*;
use log::{debug, info, warn};
use regex::RegexSet;
use std::collections::HashMap;
use std::convert::identity;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec::Vec;
use tokio::fs;

use crate::base64;
use crate::config::Config;
use crate::constants::*;
use crate::crypto;
use crate::error::Result;
use crate::merge_tree;
use crate::merge_tree::MergeOption;
use crate::symlink::Symlink;
use crate::track_file::Track;
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
    let envs = [(PACK_NAME_ENV, pack_name)];
    if let Some(command) = &config.init {
        command.exec_async(pack.deref(), envs).await?;
    }

    Ok(())
}

/// remove packages
async fn install_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = pack
        .as_ref()
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack_name}: target is none, skip install links");
            return Ok(());
        }
        Some(target) => target.clone(),
    };

    // if trace file has exists, then then pack has been installed
    let context_map: HashMap<_, _> = vec![(PACK_NAME_ENV, pack_name)].into_iter().collect();
    let track_file =
        util::shell_expend_full_with_context(target.clone().join(PACK_TRACK_FILE), |key| {
            context_map.get(key).copied()
        })?;
    if track_file.try_exists()? {
        bail!("{pack_name}: pack has been install")
    }
    fs::create_dir_all(track_file.parent().with_context(|| {
        format!(
            "{pack_name}: failed to find track file parent, {:?}",
            track_file
        )
    })?)
    .await
    .with_context(|| {
        format!(
            "{pack_name}: failed to create track file dir, {:?}",
            track_file.parent()
        )
    })?;

    let ignore_re = config
        .ignore
        .as_ref()
        .and_then(|ignore_regexs| RegexSet::new(ignore_regexs).ok());

    let over_re = config
        .over
        .as_ref()
        .and_then(|over_regexs| RegexSet::new(over_regexs).ok());

    let mut symlinks = {
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
                bail!("check conflict: {:?}", conflicts);
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

    // if config decrypted, decrypted the file
    let decrypted_path = {
        let path = config
            .decrypted
            .as_ref()
            .and_then(|it| it.decrypted_path.as_ref());
        match path {
            Some(path) => Some(util::shell_expend_full_with_context(path, |key| {
                context_map.get(key).copied()
            })?),
            None => None,
        }
    };
    let decrypted = config
        .decrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));

    if decrypted {
        let decrypted_path = decrypted_path
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path has not been configed"))?;

        let key = {
            let key_path = config
                .decrypted
                .as_ref()
                .and_then(|it| it.key_path.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: key_path has not been configed"))?;
            let key_path = &util::shell_expend_full_with_context(key_path, |key| {
                context_map.get(key).copied()
            })?;
            if !fs::try_exists(key_path).await? {
                bail!("{pack_name}: key_path not exist");
            }
            let key_base64 = fs::read_to_string(key_path).await.with_context(|| {
                format!("{pack_name}: failed to read from key_path={:?}", key_path)
            })?;
            base64::decode(&key_base64)?
        };
        let key = key.as_slice();

        let left_boundary = {
            let s = config
                .decrypted
                .as_ref()
                .and_then(|it| it.left_boundry.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: left_boundry has not been configed"))?;
            s.as_str()
        };

        let right_boundary = {
            let s = config
                .decrypted
                .as_ref()
                .and_then(|it| it.right_boundry.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: right_boundry has not been configed"))?;
            s.as_str()
        };

        let crypted_alg = {
            let s = config
                .decrypted
                .as_ref()
                .and_then(|it| it.crypted_alg.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: crypted_alg has not been configed"))?;
            s.as_str()
        };

        if !fs::try_exists(decrypted_path).await? {
            fs::create_dir_all(decrypted_path).await.with_context(|| {
                format!(
                    "{pack_name}: failed to create track file dir, {:?}",
                    decrypted_path
                )
            })?;
        }

        let mut decrypted_file_map = vec![];
        for symlink in symlinks.iter_mut() {
            let decrypted_file_path =
                util::change_base_path(&symlink.src, pack.as_path(), decrypted_path.as_path())?;
            debug!(
                "{pack_name}: change_base_path, src={:?}, base={:?}, new_base={:?}, result={:?}",
                symlink.src,
                pack.as_path(),
                decrypted_path.as_path(),
                decrypted_file_path,
            );
            decrypted_file_map.push((symlink.src.clone(), decrypted_file_path.clone()));
            symlink.src = decrypted_file_path;
        }

        // decrypted the file
        debug!("{pack_name}: decrypted paths {decrypted_file_map:?}");
        futures::stream::iter(decrypted_file_map.into_iter().map(Ok))
            .try_for_each_concurrent(None, |(origin_file_path, decrypted_file_path)| async move {
                if decrypted_file_path.try_exists()? {
                    if decrypted_file_path.is_file() || decrypted_file_path.is_symlink() {
                        fs::remove_file(&decrypted_file_path).await?;
                    } else {
                        fs::remove_dir_all(&decrypted_file_path).await?;
                    }
                }
                info!(
                    "{pack_name}: decrypt {:?} to {:?}",
                    origin_file_path, decrypted_file_path
                );
                let content = fs::read_to_string(origin_file_path).await?;
                let origin_content = crypto::decrypt_inline(
                    &content,
                    crypted_alg,
                    key,
                    left_boundary,
                    right_boundary,
                    true,
                )?;
                fs::write(&decrypted_file_path, origin_content)
                    .await
                    .with_context(|| {
                        format!(
                            "{pack_name}: failed to write decrypted_content to path={:?}",
                            &decrypted_file_path
                        )
                    })?;
                Result::<(), anyhow::Error>::Ok(())
            })
            .await?;
    }

    debug!("{pack_name}: install paths {symlinks:?}");
    futures::stream::iter(symlinks.clone().into_iter().map(Ok))
        .try_for_each_concurrent(None, |symlink| async move {
            if symlink.dst.try_exists()? {
                // the dir is empty or override regex matched
                if symlink.dst.is_file() || symlink.dst.is_symlink() {
                    fs::remove_file(&symlink.dst).await?;
                } else {
                    fs::remove_dir_all(&symlink.dst).await?;
                }
            }
            info!("install {symlink:?}");
            symlink.create().await
        })
        .await?;

    debug!("{pack_name}: installed links record to track file, track_file = {track_file:?}, links = {symlinks:?}");
    fs::write(
        track_file,
        toml::to_string_pretty(&Track {
            decrypted_path: if decrypted { decrypted_path } else { None },
            links: symlinks,
        })?,
    )
    .await?;
    Ok(())
}

/// remove packages
pub(crate) async fn clean<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = pack
        .file_name()
        .ok_or_else(|| anyhow!("path error: {pack:?}"))?;
    info!("clean pack: {pack_name:?}");

    clean_link(&config, &pack).await?;

    // execute the clear script
    let envs = [(PACK_NAME_ENV, pack_name)];
    if let Some(command) = &config.clear {
        command.exec_async(pack.deref(), envs).await?;
    }

    Ok(())
}

/// remove links
async fn clean_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack_name}: target is none, skip clean links");
            return Ok(());
        }
        Some(target) => target.clone(),
    };
    let symlinks = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || util::find_prefix_symlink(target, pack.deref()))
            .await??
    };

    debug!("{pack_name}: clean paths: {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(None, |symlink| async move {
            info!("clean {symlink:?}");
            symlink.remove().await
        })
        .await?;

    // obtain the decryption path from the configuration file
    // if decrypted remove the decrypted dir
    if config
        .decrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity))
    {
        let decrypted_path = config
            .decrypted
            .as_ref()
            .unwrap()
            .decrypted_path
            .as_ref()
            .unwrap();
        let context_map: HashMap<_, _> = vec![(PACK_NAME_ENV, pack_name)].into_iter().collect();
        let decrypted_path = util::shell_expend_full_with_context(decrypted_path, |key| {
            context_map.get(key).copied()
        })?;
        if fs::try_exists(decrypted_path.as_path()).await? {
            debug!("{pack_name}: clean decrypted dir, {decrypted_path:?}");
            fs::remove_dir_all(decrypted_path).await?;
        }
    }

    Ok(())
}

/// remove packages
pub(crate) async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(fs::canonicalize(pack.as_ref()).await?);
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    info!("remove pack: {pack_name:?}");

    remove_link(&config, &pack).await?;

    // execute the clear script
    let envs = [(PACK_NAME_ENV, pack_name)];
    if let Some(command) = &config.clear {
        command.exec_async(pack.deref(), envs).await?;
    }

    Ok(())
}

/// remove links
async fn remove_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack_name}: target is none, skip remove links");
            return Ok(());
        }
        Some(target) => target.clone(),
    };

    let context_map: HashMap<_, _> = vec![(PACK_NAME_ENV, pack_name)].into_iter().collect();
    let track_file =
        util::shell_expend_full_with_context(target.clone().join(PACK_TRACK_FILE), |key| {
            context_map.get(key).copied()
        })?;

    if !track_file.try_exists()? {
        bail!("{pack_name} has not been installed")
    }

    let track: Track = toml::from_str(fs::read_to_string(track_file.as_path()).await?.as_str())?;

    let symlinks = track.links;

    debug!("{pack_name}: remove {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(None, |symlink| async move {
            info!("remove {symlink:?}");
            symlink.remove().await
        })
        .await?;

    // obtain the decryption path from the track file
    // if is decrypted, delete the decrypted file
    if let Some(path) = track.decrypted_path {
        if fs::try_exists(path.as_path()).await? {
            debug!("{pack_name}: remove decrypted dir, {path:?}");
            fs::remove_dir_all(path).await?;
        }
    }
    fs::remove_file(track_file).await?;

    Ok(())
}
