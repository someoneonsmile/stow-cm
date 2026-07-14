use std::ops::Not;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use futures::prelude::*;
use log::{debug, info, warn};
use tokio::fs;
use walkdir::WalkDir;

use crate::config::{Config, EncryptedParams};
use crate::crypto;
use crate::error::Result;
use crate::util;

type CryptoFn = fn(&str, &str, &[u8], &str, &str, bool) -> crate::error::Result<String>;

/// 提取 encrypt/decrypt 共享的加密配置参数，执行文件扫描和流式处理。
async fn crypto_process<P: AsRef<Path>>(
    config: Arc<Config>,
    pack: P,
    crypto_fn: CryptoFn,
    op_name: &str,
    content_label: &str,
) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("{op_name} pack: {pack_name}");

    let enabled = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(std::convert::identity));

    if !enabled {
        warn!("{pack_name}: pack is not enable encrypted");
        return Ok(());
    }

    let params = config
        .encrypted
        .as_ref()
        .ok_or_else(|| anyhow!("{pack_name}: encrypted config not found"))?
        .resolve(&pack_name)
        .await?;
    let EncryptedParams {
        key,
        left_boundary,
        right_boundary,
        encrypted_alg,
    } = params;
    let key = key.as_slice();

    let ignore_re = config.ignore_regex()?;

    let files = {
        let pack = pack.clone();
        tokio::task::spawn_blocking(move || {
            // walk file, expect ignore_re, skip binary file
            let files: Vec<_> = WalkDir::new(&*pack)
                .into_iter()
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = entry.path();
                    let ignore = match ignore_re.as_ref() {
                        Some(ignore_re) => path
                            .to_str()
                            .is_some_and(|path_name| ignore_re.is_match(path_name)),
                        None => false,
                    };
                    if ignore {
                        return None;
                    }
                    if path.is_file() {
                        return Some(entry);
                    }
                    None
                })
                .filter(|entry| {
                    let a = entry.path();
                    binaryornot::is_binary(a).is_ok_and(Not::not)
                })
                .collect();

            files
        })
        .await?
    };

    debug!("{pack_name}: {op_name} paths {files:?}");
    futures::stream::iter(files.into_iter().map(Ok))
        .try_for_each_concurrent(Some(util::max_concurrent_files()), |file| {
            let pack_name = pack_name.clone();
            async move {
                let path = file.path();
                info!("{pack_name}: {op_name} {}", path.display());
                let Ok(content) = fs::read_to_string(path).await else {
                    warn!("{pack_name}: {} contains not invalid utf-8", path.display());
                    return Ok(());
                };
                let processed = crypto_fn(
                    &content,
                    encrypted_alg,
                    key,
                    left_boundary,
                    right_boundary,
                    false,
                )?;
                fs::write(path, processed).await.with_context(|| {
                    format!(
                        "{pack_name}: failed to write {content_label} to path={}",
                        path.display()
                    )
                })?;
                Result::<(), anyhow::Error>::Ok(())
            }
        })
        .await?;

    Ok(())
}

/// encrypt packages
pub async fn encrypt<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    crypto_process(
        config,
        pack,
        crypto::encrypt_inline,
        "encrypt",
        "encrypted_content",
    )
    .await
}

/// decrypt packages
pub async fn decrypt<P: AsRef<Path>>(config: Arc<Config>, pack: P) -> Result<()> {
    crypto_process(
        config,
        pack,
        crypto::decrypt_inline,
        "decrypt",
        "decrypted_content",
    )
    .await
}
