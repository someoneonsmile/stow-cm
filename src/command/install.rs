use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use log::{debug, info, warn};

use crate::config::{Config, EncryptedParams};
use crate::crypto;
use crate::error::Result;
use crate::merge_tree;
use crate::merge_tree::MergeOption;
use crate::track_file::Track;
use crate::util;

use super::{pack_envs, resolve_track_file};

/// install packages
pub fn install(config: &Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("installing");

    install_link(config, &pack)?;

    // execute the init script
    if let Some(command) = &config.init {
        info!("running init script");
        command.execute(&*pack, pack_envs(&pack, &pack_name))?;
        info!("init script done");
    }

    Ok(())
}

/// install link
fn install_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("target is none, skip install links");
        return Ok(());
    };

    // if track file already exists, then the pack has been installed
    let track_file = resolve_track_file(pack, &pack_name)?;
    if track_file.try_exists()? {
        bail!("{pack_name}: pack has been install")
    }
    std::fs::create_dir_all(track_file.parent().with_context(|| {
        format!(
            "{pack_name}: failed to find track file parent, {}",
            track_file.display()
        )
    })?)
    .with_context(|| {
        format!(
            "{pack_name}: failed to create track file dir, {:?}",
            track_file.parent()
        )
    })?;

    let ignore_re = config.ignore_regex()?;
    let over_re = config.over_regex()?;

    let merge_result = merge_tree::MergeTree::new(
        target,
        pack.as_ref(),
        Some(Arc::new(MergeOption {
            ignore: ignore_re,
            over: over_re,
            fold: config.fold,
            symlink_mode: config.symlink_mode.clone(),
        })),
    )
    .merge_add()?;

    if let Some(conflicts) = merge_result.conflicts {
        bail!("check conflict: {conflicts:?}");
    }

    if let Some(expand_symlinks) = merge_result.expand_symlinks {
        // convert symlink dir to dir
        for expand_symlink in expand_symlinks {
            util::expand_symlink_dir(expand_symlink)?;
        }
    }

    let mut symlinks = merge_result.to_create_symlinks.unwrap_or_default();

    // if config decrypted, decrypted the file
    let decrypted_path = config
        .encrypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());
    let decrypted = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));

    if decrypted {
        let decrypted_path = decrypted_path
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;

        let params = config
            .encrypted
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: encrypted config not found"))?
            .resolve(&pack_name)?;
        let EncryptedParams {
            key,
            left_boundary,
            right_boundary,
            encrypted_alg,
        } = params;
        let key = key.as_slice();

        if !decrypted_path.try_exists()? {
            std::fs::create_dir_all(decrypted_path).with_context(|| {
                format!(
                    "{pack_name}: failed to create decrypted dir, {}",
                    decrypted_path.display()
                )
            })?;
        }

        let mut decrypted_file_map = vec![];
        for symlink in &mut symlinks {
            let decrypted_file_path =
                util::change_base_path(&symlink.src, pack.as_path(), decrypted_path.as_path())?;
            debug!(
                "change_base_path, src={}, base={}, new_base={}, result={}",
                symlink.src.display(),
                pack.display(),
                decrypted_path.display(),
                decrypted_file_path.display(),
            );
            decrypted_file_map.push((symlink.src.clone(), decrypted_file_path.clone()));
            symlink.src = decrypted_file_path;
        }

        // decrypted the file
        debug!("decrypted paths {decrypted_file_map:?}");
        for (origin_file_path, decrypted_file_path) in &decrypted_file_map {
            // 用 symlink_metadata 一次性获取元数据，避免多次 stat() 调用之间的 TOCTOU 竞态窗口
            match std::fs::symlink_metadata(decrypted_file_path) {
                Ok(meta) => {
                    let ft = meta.file_type();
                    if ft.is_file() || ft.is_symlink() {
                        std::fs::remove_file(decrypted_file_path)?;
                    } else if ft.is_dir() {
                        std::fs::remove_dir_all(decrypted_file_path)?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 目标不存在，无需清理
                }
                Err(e) => return Err(e.into()),
            }
            info!(
                "decrypt {} to {}",
                origin_file_path.display(),
                decrypted_file_path.display()
            );
            let content = std::fs::read_to_string(origin_file_path)?;
            let origin_content = crypto::decrypt_inline(
                &content,
                encrypted_alg,
                key,
                left_boundary,
                right_boundary,
                true,
            )?;
            std::fs::write(decrypted_file_path, origin_content).with_context(|| {
                format!(
                    "{pack_name}: failed to write decrypted content to path={}",
                    decrypted_file_path.display()
                )
            })?;
        }
    }

    debug!("install paths {symlinks:?}");
    for symlink in &symlinks {
        info!("symlink {symlink}");
        symlink.create(true)?;
    }

    debug!(
        "installed links record to track file, track_file = {}, links = {symlinks:?}",
        track_file.display()
    );
    std::fs::write(
        track_file,
        toml::to_string_pretty(&Track {
            decrypted_path: if decrypted {
                decrypted_path.cloned()
            } else {
                None
            },
            links: symlinks,
            pack_name: Some(pack_name.clone()),
            pack_path: Some((**pack).clone()),
            target: Some(target.clone()),
        })?,
    )?;
    Ok(())
}
