use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use futures::prelude::*;
use log::warn;
use maplit::hashmap;

use crate::config::Config;
use crate::constants::{CONFIG_FILE_NAME, PACK_ID_ENV, PACK_NAME_ENV};
use crate::error::Result;
use crate::util;

/// exec packages
pub async fn exec_all<F, P>(common_config: Arc<Option<Config>>, packs: Vec<P>, f: F) -> Result<()>
where
    F: AsyncFn(Arc<Config>, P) -> Result<()>,
    P: AsRef<Path>,
{
    let results = futures::stream::iter(packs)
        .map(async |pack| {
            let mut pack_config = Config::from_path(pack.as_ref().join(CONFIG_FILE_NAME))?;
            if pack_config.is_none() {
                warn!(
                    "{}: doesn't have its own config file, will use the common config file",
                    pack.as_ref().display()
                );
                // error!(
                //     "{:?} is not the pack_home (which contains {} config file)",
                //     pack.as_ref(),
                //     CONFIG_FILE_NAME
                // );
                // return Ok(None);
            }
            merge::option::recurse(&mut pack_config, common_config.deref().clone());
            let Some(mut config) = pack_config else {
                anyhow::bail!("no config")
            };
            config.normalize();

            let pack_name = config.resolve_pack_name(pack.as_ref())?;

            // let mut config = match pack_config.merge_with(|| common_config.deref().clone()) {
            //     Some(config) => config,
            //     None => unreachable!("no config"),
            // };

            let context_map = hashmap! {
                PACK_ID_ENV => util::hash(&pack.as_ref().to_string_lossy()),
                PACK_NAME_ENV => pack_name.to_string(),
            };
            config.target = config
                .target
                .as_ref()
                .map(|target| {
                    util::shell_expand_full_with_context(target, |key| context_map.get(key))
                })
                .transpose()?;
            config.encrypted = config
                .encrypted
                .map(|mut encrypted| {
                    encrypted.key_path = encrypted
                        .key_path
                        .map(|key_path| {
                            util::shell_expand_full_with_context(key_path, |key| {
                                context_map.get(key)
                            })
                        })
                        .transpose()?;

                    encrypted.decrypted_path = encrypted
                        .decrypted_path
                        .map(|decrypted_path| {
                            util::shell_expand_full_with_context(decrypted_path, |key| {
                                context_map.get(key)
                            })
                        })
                        .transpose()?;
                    anyhow::Ok(encrypted)
                })
                .transpose()?;
            f(Arc::new(config), pack).await?;
            anyhow::Ok(())
        })
        .buffer_unordered(util::max_concurrent_packs())
        .collect::<Vec<Result<()>>>()
        .await;
    // 收集所有错误并统一返回，避免静默吞噬
    let mut errors = Vec::new();
    for result in results {
        if let Err(e) = result {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{} pack(s) failed:\n{}",
            errors.len(),
            errors
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}
