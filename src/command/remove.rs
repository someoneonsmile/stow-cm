use std::path::{Path, PathBuf};
use std::sync::Arc;

use futures::prelude::*;
use log::{debug, info, warn};
use tokio::fs;

use crate::config::Config;
use crate::error::Result;
use crate::track_file::Track;
use crate::util;

use super::{pack_envs, resolve_track_file};

/// remove packages
pub async fn remove<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = util::pack_name(&pack)?;
    info!("remove pack: {pack_name}");

    remove_link(&config, &pack).await?;

    // execute the clear script
    if let Some(command) = &config.clear {
        command
            .exec_async(&*pack, pack_envs(&pack, pack_name))
            .await?;
    }

    Ok(())
}

/// remove links
async fn remove_link(_config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = util::pack_name(pack)?;

    let track_file = resolve_track_file(pack, pack_name)?;

    if !track_file.try_exists()? {
        warn!("{pack_name}: there is no link is installed");
        return Ok(());
    }

    let track: Track = toml::from_str(fs::read_to_string(track_file.as_path()).await?.as_str())?;

    let symlinks = track.links;

    debug!("{pack_name}: remove {symlinks:?}");
    futures::stream::iter(symlinks.into_iter().map(Ok))
        .try_for_each_concurrent(Some(util::max_concurrent_files()), |symlink| async move {
            info!("{pack_name}: remove symlink {symlink}");
            symlink.remove().await
        })
        .await?;

    // obtain the decryption path from the track file
    // if is decrypted, delete the decrypted file
    if let Some(path) = track.decrypted_path
        && fs::try_exists(path.as_path()).await?
    {
        debug!("{pack_name}: remove decrypted dir, {}", path.display());
        fs::remove_dir_all(path).await?;
    }

    fs::remove_file(track_file).await?;

    Ok(())
}
