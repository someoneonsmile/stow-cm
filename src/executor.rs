use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use futures::prelude::*;

use crate::config::Config;
use crate::error::Result;
use crate::util;

pub async fn exec_all<F, P>(common_config: Arc<Option<Config>>, packs: Vec<P>, f: F) -> Result<()>
where
    F: AsyncFn(Arc<Config>, P) -> Result<()>,
    P: AsRef<Path>,
{
    let global = common_config
        .deref()
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("global config not loaded"))?;
    let results = futures::stream::iter(packs)
        .map(async |pack| {
            let config = Config::for_pack(pack.as_ref(), global, None, false)?;
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
