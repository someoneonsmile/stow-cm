use crate::merge::MergeWith;
use futures::prelude::*;
use log::error;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::vec::Vec;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::constants::*;
use crate::error::Result;

use crate::util;

/// exec packages
pub(crate) async fn exec_all<F, P, Fut>(
    common_config: Arc<Option<Config>>,
    packs: Vec<P>,
    f: F,
) -> Result<()>
where
    F: Fn(Arc<Config>, P) -> Fut,
    P: AsRef<Path>,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    futures::stream::iter(packs.into_iter().map(Ok))
        .try_filter_map(|pack| async {
            let pack_config = Config::from_path(pack.as_ref().join(CONFIG_FILE_NAME))?;
            if pack_config.is_none() {
                error!(
                    "{:?} is not the pack_home (which contains {} config file)",
                    pack.as_ref(),
                    CONFIG_FILE_NAME
                );
                return Ok(None);
            };
            let pack_name = pack
                .as_ref()
                .file_name()
                .and_then(|it| it.to_str())
                .ok_or_else(|| anyhow::anyhow!("path error: {:?}", pack.as_ref()))?;
            let mut config = match pack_config.merge_with(|| common_config.deref().clone()) {
                Some(config) => config,
                None => unreachable!("no config"),
            };

            let context_map: HashMap<_, _> = vec![(PACK_NAME_ENV, pack_name)].into_iter().collect();
            config.target = match config.target.as_mut() {
                Some(target) => Some(util::shell_expend_full_with_context(target, |key| {
                    // context_map.get(key).map(ToOwned::to_owned)
                    context_map.get(key).copied()
                })?),
                None => None,
            };
            let fut = tokio::spawn((f)(Arc::new(config), pack));
            Ok(Some(fut)) as Result<Option<JoinHandle<Result<()>>>>
        })
        .try_for_each_concurrent(None, |future| async move {
            let _ = future.await;
            Ok(())
        })
        .await?;

    Ok(())
}
