use crate::merge::MergeWith;
use futures::prelude::*;
use log::{error, warn};
use maplit::hashmap;
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;
use std::vec::Vec;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::constants::{CONFIG_FILE_NAME, PACK_ID_ENV, PACK_NAME_ENV};
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
                warn!(
                    "{:?}: doesn't have its own config file, will use the common config file",
                    pack.as_ref()
                );
                // error!(
                //     "{:?} is not the pack_home (which contains {} config file)",
                //     pack.as_ref(),
                //     CONFIG_FILE_NAME
                // );
                // return Ok(None);
            };
            let pack_name = pack
                .as_ref()
                .file_name()
                .and_then(|it| it.to_str())
                .ok_or_else(|| anyhow::anyhow!("path error: {:?}", pack.as_ref()))?;
            // TODO:
            let mut config = match pack_config.merge_with(|| common_config.deref().clone()) {
                Some(config) => config,
                None => unreachable!("no config"),
            };

            let context_map = hashmap! {
                PACK_ID_ENV => util::hash(&pack.as_ref().to_string_lossy()),
                PACK_NAME_ENV => pack_name.to_owned(),
            };
            config.target = match config.target.as_ref() {
                Some(target) => Some(util::shell_expand_full_with_context(target, |key| {
                    context_map.get(key)
                })?),
                None => None,
            };
            config.crypted = match config.crypted {
                Some(crypted) => {
                    let mut crypted = crypted;
                    crypted.key_path = match crypted.key_path {
                        Some(key_path) => {
                            Some(util::shell_expand_full_with_context(key_path, |key| {
                                context_map.get(key)
                            })?)
                        }
                        None => None,
                    };
                    crypted.decrypted_path = match crypted.decrypted_path {
                        Some(decrypted_path) => Some(util::shell_expand_full_with_context(
                            decrypted_path,
                            |key| context_map.get(key),
                        )?),
                        None => None,
                    };
                    Some(crypted)
                }
                None => None,
            };
            let fut = tokio::spawn((f)(Arc::new(config), pack));
            Ok(Some(fut)) as Result<Option<JoinHandle<Result<()>>>>
        })
        .try_for_each_concurrent(None, |future| async move {
            let rr = future.await;
            match rr {
                Ok(Err(err)) => {
                    error!("{:?}", err);
                }
                Err(err) => {
                    error!("{:?}", err);
                }
                _ => {}
            };

            Ok(())
        })
        .await?;

    Ok(())
}
