use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use futures::prelude::*;
use log::{debug, info, warn};
use maplit::hashmap;
use regex::RegexSet;
use std::convert::identity;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::vec::Vec;
use tokio::fs;
use walkdir::WalkDir;

use crate::base64;
use crate::config::Config;
use crate::constants::{PACK_ID_ENV, PACK_NAME_ENV, PACK_TRACK_FILE};
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
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {pack:?}"))?;
    info!("install pack: {pack_name}");

    install_link(&config, &pack).await?;

    // execute the init script
    if let Some(command) = &config.init {
        let envs = [
            (PACK_ID_ENV, util::hash(&pack.to_string_lossy())),
            (PACK_NAME_ENV, pack_name.to_owned()),
        ];
        command.exec_async(&*pack, envs).await?;
    }

    Ok(())
}

/// install link
async fn install_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    let target = match config.target.as_ref() {
        None => {
            warn!("{pack_name}: target is none, skip install links");
            return Ok(());
        }
        Some(target) => target,
    };

    // if trace file has exists, then then pack has been installed
    let context_map = hashmap! {
        PACK_ID_ENV => util::hash(&pack.as_ref().to_string_lossy()),
        PACK_NAME_ENV => pack_name.to_owned(),
    };
    let track_file =
        util::shell_expand_full_with_context(PACK_TRACK_FILE, |key| context_map.get(key))?;
    if track_file.try_exists()? {
        bail!("{pack_name}: pack has been install")
    }
    fs::create_dir_all(track_file.parent().with_context(|| {
        format!("{pack_name}: failed to find track file parent, {track_file:?}")
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
        .map(RegexSet::new)
        .transpose()
        .with_context(|| anyhow!("{:?}", config.ignore))?;

    let over_re = config
        .over
        .as_ref()
        .map(RegexSet::new)
        .transpose()
        .with_context(|| anyhow!("{:?}", config.over))?;

    let mut symlinks = {
        let pack = pack.clone();
        let config = config.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || {
            let merge_result = merge_tree::MergeTree::new(
                target,
                &*pack,
                Some(Arc::new(MergeOption {
                    ignore: ignore_re,
                    over: over_re,
                    fold: config.fold,
                    symlink_mode: config.symlink_mode.clone(),
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
    let decrypted_path = config
        .crypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());
    let decrypted = config
        .crypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));

    if decrypted {
        let decrypted_path = decrypted_path
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;

        let key = {
            let key_path = config
                .crypted
                .as_ref()
                .and_then(|it| it.key_path.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: key_path is not configured"))?;
            if !fs::try_exists(key_path).await? {
                bail!("{pack_name}: key_path not exist");
            }
            let key_base64 = fs::read_to_string(key_path).await.with_context(|| {
                format!("{pack_name}: failed to read from key_path={key_path:?}")
            })?;
            base64::decode(&key_base64)?
        };
        let key = key.as_slice();

        let left_boundary = {
            let s = config
                .crypted
                .as_ref()
                .and_then(|it| it.left_boundry.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: left_boundry is not configured"))?;
            s.as_str()
        };

        let right_boundary = {
            let s = config
                .crypted
                .as_ref()
                .and_then(|it| it.right_boundry.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: right_boundry is not configured"))?;
            s.as_str()
        };

        let crypted_alg = {
            let s = config
                .crypted
                .as_ref()
                .and_then(|it| it.crypted_alg.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: crypted_alg is not configured"))?;
            s.as_str()
        };

        if !fs::try_exists(decrypted_path).await? {
            fs::create_dir_all(decrypted_path).await.with_context(|| {
                format!(
                    // FIX: tip track file?
                    "{pack_name}: failed to create track file dir, {decrypted_path:?}"
                )
            })?;
        }

        let mut decrypted_file_map = vec![];
        for symlink in &mut symlinks {
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
                            "{pack_name}: failed to write decrypted content to path={:?}",
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
            info!("{pack_name}: symlink {symlink}");
            symlink.create(true).await
        })
        .await?;

    debug!("{pack_name}: installed links record to track file, track_file = {track_file:?}, links = {symlinks:?}");
    fs::write(
        track_file,
        toml::to_string_pretty(&Track {
            decrypted_path: if decrypted {
                decrypted_path.cloned()
            } else {
                None
            },
            links: symlinks,
        })?,
    )
    .await?;
    Ok(())
}

/// clean packages
pub(crate) async fn clean<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {pack:?}"))?;
    info!("clean pack: {pack_name}");

    clean_link(&config, &pack).await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        let envs = [
            (PACK_ID_ENV, util::hash(&pack.to_string_lossy())),
            (PACK_NAME_ENV, pack_name.to_owned()),
        ];
        command.exec_async(&*pack, envs).await?;
    }

    Ok(())
}

/// clean links
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
        Some(target) => target,
    };
    let symlinks = {
        let pack = pack.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || util::find_prefix_symlink(target, &*pack)).await??
    };

    debug!("{pack_name}: clean paths: {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(None, |symlink| async move {
            info!("{pack_name}: remove symlink {symlink}");
            symlink.remove().await
        })
        .await?;

    // obtain the decryption path from the configuration file
    // if decrypted remove the decrypted dir
    let decrypted_path = config
        .crypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());
    let crypted = config
        .crypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    if crypted {
        let decrypted_path = decrypted_path
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;
        if fs::try_exists(decrypted_path.as_path()).await? {
            info!("{pack_name}: clean decrypted dir, {decrypted_path:?}");
            fs::remove_dir_all(decrypted_path).await?;
        }
    }

    Ok(())
}

/// remove packages
pub(crate) async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    info!("remove pack: {pack_name:?}");

    remove_link(&config, &pack).await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        let envs = [
            (PACK_ID_ENV, util::hash(&pack.to_string_lossy())),
            (PACK_NAME_ENV, pack_name.to_owned()),
        ];
        command.exec_async(&*pack, envs).await?;
    }

    Ok(())
}

/// remove links
async fn remove_link(_config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    // NOTE: not need because track file move from target dir to $XDG_STATE_HOME
    // let target = match config.target.as_ref() {
    //     None => {
    //         warn!("{pack_name}: target is none, skip remove links");
    //         return Ok(());
    //     }
    //     Some(target) => target,
    // };

    let context_map = hashmap! {
        PACK_ID_ENV => util::hash(&pack.as_ref().to_string_lossy()),
        PACK_NAME_ENV => pack_name.to_owned(),
    };
    let track_file =
        util::shell_expand_full_with_context(PACK_TRACK_FILE, |key| context_map.get(key))?;

    if !track_file.try_exists()? {
        warn!("{pack_name}: there is no link is installed");
        return Ok(());
    }

    let track: Track = toml::from_str(fs::read_to_string(track_file.as_path()).await?.as_str())?;

    let symlinks = track.links;

    debug!("{pack_name}: remove {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(None, |symlink| async move {
            info!("{pack_name}: remove symlink {symlink}");
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

/// encrypt packages
pub(crate) async fn encrypt<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    info!("encrypt pack: {pack_name:?}");

    let decrypted = config
        .crypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));

    if !decrypted {
        warn!("{pack_name}: pack is not enable crypted");
        return Ok(());
    }

    let key = {
        let key_path = config
            .crypted
            .as_ref()
            .and_then(|it| it.key_path.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: key_path is not configured"))?;
        if !fs::try_exists(key_path).await? {
            bail!("{pack_name}: key_path not exist");
        }
        let key_base64 = fs::read_to_string(key_path)
            .await
            .with_context(|| format!("{pack_name}: failed to read from key_path={key_path:?}"))?;
        base64::decode(&key_base64)?
    };
    let key = key.as_slice();

    let left_boundary = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.left_boundry.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: left_boundry is not configured"))?;
        s.as_str()
    };

    let right_boundary = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.right_boundry.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: right_boundry is not configured"))?;
        s.as_str()
    };

    let crypted_alg = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.crypted_alg.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: crypted_alg is not configured"))?;
        s.as_str()
    };

    let ignore_re = config
        .ignore
        .as_ref()
        .map(RegexSet::new)
        .transpose()
        .with_context(|| anyhow!("{:?}", config.ignore))?;

    let files = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || {
            // walk file, expect ignore_re, skip binary file
            let files: Vec<_> = WalkDir::new(&*pack)
                .into_iter()
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    let ignore = match ignore_re.as_ref() {
                        Some(ignore_re) => path
                            .to_str()
                            .is_some_and(|path_name| ignore_re.is_match(path_name)),
                        None => true,
                    };
                    if ignore {
                        return None;
                    }
                    if path.is_file() {
                        return Some(entry);
                    }
                    None
                })
                .collect();

            files
        })
        .await?
    };

    // encrypt the file
    debug!("{pack_name}: encrypt paths {files:?}");
    futures::stream::iter(files.into_iter().map(Ok))
        .try_for_each_concurrent(None, |file| async move {
            let path = file.path();
            info!("{pack_name}: encrypt {:?}", path);
            let Ok(content) = fs::read_to_string(path).await else {
                warn!("{pack_name}: {:?} contains not invalid utf-8", path);
                return Ok(());
            };
            let encrypted_content = crypto::encrypt_inline(
                &content,
                crypted_alg,
                key,
                left_boundary,
                right_boundary,
                false,
            )?;
            fs::write(path, encrypted_content).await.with_context(|| {
                format!("{pack_name}: failed to write encrypted_content to path={path:?}")
            })?;
            Result::<(), anyhow::Error>::Ok(())
        })
        .await?;

    Ok(())
}

/// decrypt packages
pub(crate) async fn decrypt<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = pack
        .file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {:?}", pack.as_ref()))?;
    info!("decrypt pack: {pack_name:?}");

    let decrypted = config
        .crypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));

    if !decrypted {
        warn!("{pack_name}: pack is not enable crypted");
        return Ok(());
    }

    let key = {
        let key_path = config
            .crypted
            .as_ref()
            .and_then(|it| it.key_path.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: key_path is not configured"))?;
        if !fs::try_exists(key_path).await? {
            bail!("{pack_name}: key_path not exist");
        }
        let key_base64 = fs::read_to_string(key_path)
            .await
            .with_context(|| format!("{pack_name}: failed to read from key_path={key_path:?}"))?;
        base64::decode(&key_base64)?
    };
    let key = key.as_slice();

    let left_boundary = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.left_boundry.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: left_boundry is not configured"))?;
        s.as_str()
    };

    let right_boundary = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.right_boundry.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: right_boundry is not configured"))?;
        s.as_str()
    };

    let crypted_alg = {
        let s = config
            .crypted
            .as_ref()
            .and_then(|it| it.crypted_alg.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: crypted_alg is not configured"))?;
        s.as_str()
    };

    let ignore_re = config
        .ignore
        .as_ref()
        .map(RegexSet::new)
        .transpose()
        .with_context(|| anyhow!("{:?}", config.ignore))?;

    let files = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || {
            // walk file, expect ignore_re, skip binary file
            let files: Vec<_> = WalkDir::new(&*pack)
                .into_iter()
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    let ignore = match ignore_re.as_ref() {
                        Some(ignore_re) => path
                            .to_str()
                            .is_some_and(|path_name| ignore_re.is_match(path_name)),
                        None => true,
                    };
                    if ignore {
                        return None;
                    }
                    if path.is_file() {
                        return Some(entry);
                    }
                    None
                })
                .collect();

            files
        })
        .await?
    };

    // decrypt the file
    debug!("{pack_name}: decrypt paths {files:?}");
    futures::stream::iter(files.into_iter().map(Ok))
        .try_for_each_concurrent(None, |file| async move {
            let path = file.path();
            info!("{pack_name}: decrypt {:?}", path);
            let Ok(content) = fs::read_to_string(path).await else {
                warn!("{pack_name}: {:?} contains not invalid utf-8", path);
                return Ok(());
            };
            let decrypted_content = crypto::decrypt_inline(
                &content,
                crypted_alg,
                key,
                left_boundary,
                right_boundary,
                false,
            )?;
            fs::write(path, decrypted_content).await.with_context(|| {
                format!("{pack_name}: failed to write decrypted_content to path={path:?}")
            })?;
            Result::<(), anyhow::Error>::Ok(())
        })
        .await?;

    Ok(())
}
