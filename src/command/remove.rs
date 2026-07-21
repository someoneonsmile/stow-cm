use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{debug, info, warn};

use crate::config::Config;
use crate::error::Result;
use crate::track_file::Track;

use super::{pack_envs, resolve_track_file};

/// remove packages
pub fn remove<P: AsRef<Path>>(config: &Arc<Config>, pack: P) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("remove pack: {pack_name}");

    remove_link(config, &pack)?;

    // execute the clear script
    if let Some(command) = &config.clear {
        command.execute(&*pack, pack_envs(&pack, &pack_name))?;
    }

    Ok(())
}

/// remove links
fn remove_link(config: &Arc<Config>, pack: &Arc<PathBuf>) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();

    let track_file = resolve_track_file(pack, &pack_name)?;

    if !track_file.try_exists()? {
        warn!("{pack_name}: there is no link is installed");
        return Ok(());
    }

    let track: Track = toml::from_str(std::fs::read_to_string(track_file.as_path())?.as_str())?;

    let symlinks = track.links;

    debug!("{pack_name}: remove {symlinks:?}");
    for symlink in &symlinks {
        info!("{pack_name}: remove symlink {symlink}");
        symlink.remove()?;
    }

    // obtain the decryption path from the track file
    // if is decrypted, delete the decrypted file
    if let Some(path) = track.decrypted_path
        && path.try_exists()?
    {
        debug!("{pack_name}: remove decrypted dir, {}", path.display());
        std::fs::remove_dir_all(path)?;
    }

    std::fs::remove_file(track_file)?;

    Ok(())
}
