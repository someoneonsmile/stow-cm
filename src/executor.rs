use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use crate::config::Config;
use crate::error::Result;

pub fn exec_all<F, P>(common_config: &Arc<Option<Config>>, packs: Vec<P>, f: F) -> Result<()>
where
    F: Fn(&Arc<Config>, P) -> Result<()>,
    P: AsRef<Path>,
{
    let global = common_config
        .deref()
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("global config not loaded"))?;
    let mut errors = Vec::new();
    for pack in packs {
        let config = match Config::for_pack(pack.as_ref(), global, None, false) {
            Ok(c) => c,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if let Err(e) = f(&Arc::new(config), pack) {
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
