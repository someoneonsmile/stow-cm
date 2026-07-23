use std::path::{Path, PathBuf};
use std::sync::Arc;

use log::{debug, info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::Config;
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::track_file::Track;

/// remove packages
pub fn remove<P: AsRef<Path>>(config: &Arc<Config>, pack: P, dry_run: bool) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("removing");

    remove_link(&pack, dry_run)?;

    // execute the clear script
    if let Some(command) = &config.clear {
        if dry_run {
            info!("would run clear script (dry-run)");
        } else {
            info!("running clear script");
            command.execute(&*pack, pack_envs(&pack, &pack_name))?;
            info!("clear script done");
        }
    }

    Ok(())
}

/// remove links
fn remove_link(pack: &Arc<PathBuf>, dry_run: bool) -> Result<()> {
    let track_file = resolve_track_file(pack)?;

    if !track_file.try_exists()? {
        warn!("no links installed");
        return Ok(());
    }

    let track: Track = toml::from_str(&std::fs::read_to_string(track_file.as_path())?)?;
    let symlinks = track.links.clone();

    // plan_clean iterates track.links and creates RemoveLink actions — no virtual tree needed
    let plan = planner::plan_clean(&track);

    debug!("remove {symlinks:?}");
    executor::execute_plan(&plan, dry_run)?;

    // obtain the decryption path from the track file
    // if is decrypted, delete the decrypted file
    if let Some(path) = track.decrypted_path
        && path.try_exists()?
    {
        if dry_run {
            info!("would remove decrypted dir: {}", path.display());
        } else {
            debug!("remove decrypted dir, {}", path.display());
            std::fs::remove_dir_all(path)?;
        }
    }

    if dry_run {
        info!("would remove track file: {}", track_file.display());
    } else {
        std::fs::remove_file(track_file)?;
    }

    Ok(())
}
