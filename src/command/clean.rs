use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use log::{debug, info, warn};

use crate::config::Config;
use crate::error::Result;
use crate::util;

use super::{pack_envs, resolve_track_file};

/// clean packages
pub fn clean<P: AsRef<Path>>(config: &Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("cleaning");

    clean_link(config, &pack)?;

    // execute the clear script
    if let Some(command) = &config.clear {
        info!("running clear script");
        command.execute(&*pack, pack_envs(&pack, &pack_name))?;
        info!("clear script done");
    }

    Ok(())
}

/// clean links
fn clean_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("target is none, skip clean links");
        return Ok(());
    };
    let symlinks = util::find_prefix_symlink(target, pack.as_ref())?;

    debug!("clean paths: {symlinks:?}");
    for symlink in &symlinks {
        info!("remove symlink {symlink}");
        symlink.remove()?;
    }

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
        if decrypted_path.try_exists()? {
            info!("clean decrypted dir, {}", decrypted_path.display());
            std::fs::remove_dir_all(decrypted_path)?;
        }
    }

    // 清理完成后删除残留的 track 文件，保持状态一致
    let track_file = resolve_track_file(pack, &pack_name)?;
    if track_file.try_exists()? {
        debug!("clean track file, {}", track_file.display());
        std::fs::remove_file(track_file)?;
    }

    Ok(())
}
