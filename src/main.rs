use log::debug;

use std::sync::Arc;

use crate::cli::Opt;
use crate::command::install;
use crate::command::reload;
use crate::command::remove;
use crate::command::unlink;
use crate::config::Config;
use crate::constants::*;
use crate::error::Result;
use crate::merge::Merge;

mod base64;
mod cli;
mod command;
mod config;
mod constants;
mod crypto;
mod custom_type;
mod error;
mod executor;
mod merge;
mod merge_tree;
mod symlink;
mod track_file;
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

    let common_config = Config::from_path(GLOBAL_CONFIG_FILE)?;
    let common_config = common_config.merge(Some(Default::default()));
    let common_config = Arc::new(common_config);

    debug!("common_config: {common_config:?}");

    if let Some(to_unlink) = opt.to_unlink {
        let common_config = common_config.clone();
        executor::exec_all(common_config, to_unlink, unlink).await?;
    }
    if let Some(to_remove) = opt.to_remove {
        let common_config = common_config.clone();
        executor::exec_all(common_config, to_remove, remove).await?;
    }
    if let Some(to_install) = opt.to_install {
        let common_config = common_config.clone();
        executor::exec_all(common_config, to_install, install).await?;
    }
    if let Some(to_reload) = opt.to_reload {
        let common_config = common_config.clone();
        executor::exec_all(common_config, to_reload, reload).await?;
    }

    Ok(())
}
