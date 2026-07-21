use std::ops::Not;
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use log::{debug, info, warn};
use walkdir::WalkDir;

use crate::config::{Config, EncryptedParams};
use crate::crypto;
use crate::error::Result;

type CryptoFn = fn(&str, &str, &[u8], &str, &str, bool) -> crate::error::Result<String>;

/// 提取 encrypt/decrypt 共享的加密配置参数，执行文件扫描和流式处理。
fn crypto_process<P: AsRef<Path>>(
    config: &Arc<Config>,
    pack: P,
    crypto_fn: CryptoFn,
    op_name: &str,
    content_label: &str,
) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("{op_name}");

    let enabled = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(std::convert::identity));

    if !enabled {
        warn!("pack is not enable encrypted");
        return Ok(());
    }

    let params = config
        .encrypted
        .as_ref()
        .ok_or_else(|| anyhow!("{pack_name}: encrypted config not found"))?
        .resolve(&pack_name)?;
    let EncryptedParams {
        key,
        left_boundary,
        right_boundary,
        encrypted_alg,
    } = params;
    let key = key.as_slice();

    let ignore_re = config.ignore_regex()?;

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

    debug!("{op_name} paths {files:?}");
    for file in &files {
        let path = file.path();
        info!("{op_name} {}", path.display());
        let Ok(content) = std::fs::read_to_string(path) else {
            warn!("{} contains not invalid utf-8", path.display());
            continue;
        };
        let processed = crypto_fn(
            &content,
            encrypted_alg,
            key,
            left_boundary,
            right_boundary,
            false,
        )?;
        std::fs::write(path, processed).with_context(|| {
            format!(
                "{pack_name}: failed to write {content_label} to path={}",
                path.display()
            )
        })?;
    }

    Ok(())
}

/// encrypt packages
pub fn encrypt<P: AsRef<Path>>(config: &Arc<Config>, pack: P) -> Result<()> {
    crypto_process(
        config,
        pack,
        crypto::encrypt_inline,
        "encrypt",
        "encrypted_content",
    )
}

/// decrypt packages
pub fn decrypt<P: AsRef<Path>>(config: &Arc<Config>, pack: P) -> Result<()> {
    crypto_process(
        config,
        pack,
        crypto::decrypt_inline,
        "decrypt",
        "decrypted_content",
    )
}
