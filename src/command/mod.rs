mod clean;
mod crypto;
mod install;
mod remove;

pub use clean::clean;
pub use crypto::{decrypt, encrypt};
pub use install::install;
pub use remove::remove;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use maplit::hashmap;

use crate::config::Config;
use crate::constants::{PACK_ID_ENV, PACK_NAME_ENV};
use crate::error::Result;
use crate::paths::pack_track_file;
use crate::util;

/// 构造 pack 操作的环境变量 `[(PACK_ID_ENV, hash), (PACK_NAME_ENV, pack_name)]`，
/// 消除 `install`/`clean`/`remove` 中的重复注入逻辑。
pub(super) fn pack_envs(pack: &Path, pack_name: &str) -> [(&'static str, String); 2] {
    [
        (PACK_ID_ENV, util::hash(&pack.to_string_lossy())),
        (PACK_NAME_ENV, pack_name.to_owned()),
    ]
}

/// 解析 pack 对应的 track file 路径，消除 `install`/`clean`/`remove` 中的重复逻辑。
pub(super) fn resolve_track_file(pack: &Path, pack_name: &str) -> Result<PathBuf> {
    let context_map = hashmap! {
        PACK_ID_ENV => util::hash(&pack.to_string_lossy()),
        PACK_NAME_ENV => pack_name.to_owned(),
    };
    let track_file =
        util::shell_expand_full_with_context(pack_track_file(), |key| context_map.get(key))?;
    Ok(track_file)
}

/// reload packages
pub async fn reload(config: Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    remove::remove(config.clone(), &pack).await?;
    install::install(config, &pack).await?;
    Ok(())
}
