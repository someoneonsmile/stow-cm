use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use futures::prelude::*;
use log::{debug, info, warn};
use tokio::fs;

use crate::config::Config;
use crate::error::Result;
use crate::util;

use super::{pack_envs, resolve_track_file};

/// clean packages
pub async fn clean<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("clean pack: {pack_name}");

    clean_link(&config, &pack).await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        command
            .exec_async(&*pack, pack_envs(&pack, &pack_name))
            .await?;
    }

    Ok(())
}

/// clean links
async fn clean_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("{pack_name}: target is none, skip clean links");
        return Ok(());
    };
    let symlinks = {
        let pack = pack.clone();
        let target = target.clone();
        tokio::task::spawn_blocking(move || util::find_prefix_symlink(target, &*pack)).await??
    };

    debug!("{pack_name}: clean paths: {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(Some(util::max_concurrent_files()), |symlink| {
            let pack_name = pack_name.clone();
            async move {
                info!("{pack_name}: remove symlink {symlink}");
                symlink.remove().await
            }
        })
        .await?;

    // obtain the decryption path from the configuration file
    // if decrypted remove the decrypted dir
    let decrypted_path = config
        .encrypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());
    let encrypted = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    if encrypted {
        let decrypted_path = decrypted_path
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;
        if fs::try_exists(decrypted_path.as_path()).await? {
            info!(
                "{pack_name}: clean decrypted dir, {}",
                decrypted_path.display()
            );
            fs::remove_dir_all(decrypted_path).await?;
        }
    }

    // 清理完成后删除残留的 track 文件，保持状态一致
    let track_file = resolve_track_file(pack, &pack_name)?;
    if track_file.try_exists()? {
        debug!("{pack_name}: clean track file, {}", track_file.display());
        fs::remove_file(track_file).await?;
    }

    Ok(())
}
