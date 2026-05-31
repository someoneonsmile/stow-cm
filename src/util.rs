use std::env::VarError;
use std::path::{Path, PathBuf};

use anyhow::Context;
use futures::prelude::*;
use sha3::{Digest, Sha3_256};
use shellexpand::LookupError;
use tokio::fs;
use walkdir::WalkDir;

use crate::error::{Result, anyhow};
use crate::symlink::{Symlink, SymlinkMode};

pub fn shell_expand_full<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let path = path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow!("path error"))?;
    Ok(PathBuf::from(
        shellexpand::tilde(shellexpand::full(path)?.as_ref()).as_ref(),
    ))
}

pub fn shell_expand_full_with_context<P, C, S>(path: P, context: C) -> Result<PathBuf>
where
    P: AsRef<Path>,
    C: Fn(&str) -> Option<S>,
    S: Into<String>,
{
    let path = path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow!("path error"))?;
    Ok(PathBuf::from(
        shellexpand::tilde(
            shellexpand::env_with_context(path, |key| {
                std::result::Result::<Option<String>, LookupError<VarError>>::Ok(
                    context(key)
                        .map(std::convert::Into::into)
                        .or_else(|| std::env::var(key).ok()),
                )
            })?
            .as_ref(),
        )
        .as_ref(),
    ))
}

/// expand the dir and symlink the subpath under the dir
pub fn expand_symlink_dir(expand_symlink: impl AsRef<Path>) -> Result<()> {
    let sub_paths = std::fs::read_dir(&expand_symlink)?;
    let point_to = std::fs::read_link(&expand_symlink)?;
    std::fs::remove_file(&expand_symlink)?;
    std::fs::create_dir_all(&expand_symlink)?;
    for sub_path in sub_paths {
        let sub_path = sub_path?;
        std::os::unix::fs::symlink(
            // TODO: change_base_path
            point_to.join(sub_path.path().strip_prefix(&expand_symlink)?),
            sub_path.path(),
        )?;
    }
    // TODO: return all create link
    Ok(())
}

/// just contains the dir don't has file
pub fn is_empty_dir(path: impl AsRef<Path>) -> bool {
    !path.as_ref().exists()
        || (path.as_ref().is_dir()
            && walkdir::WalkDir::new(path)
                .follow_links(true)
                .into_iter()
                .filter_entry(|e| e.file_type().is_file())
                .next()
                .is_none())
}

/// find the symlink that point to the path start with `link_prefix`
pub fn find_prefix_symlink(
    dir_path: impl AsRef<Path>,
    link_prefix: impl AsRef<Path>,
) -> Result<Vec<Symlink>> {
    let mut paths = Vec::new();
    if dir_path.as_ref().exists() {
        for entry in WalkDir::new(dir_path)
            .follow_links(false)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            let path = entry.into_path();
            if path.is_symlink() {
                let point_to = std::fs::read_link(&path)?;
                if point_to.starts_with(&link_prefix) {
                    paths.push(Symlink {
                        src: point_to,
                        dst: path,
                        mode: SymlinkMode::Symlink,
                    });
                }
            }
        }
    }
    Ok(paths)
}

/// return true if three has different sub node (empty dir exclude)
pub fn has_new_sub(a: impl AsRef<Path>, b: impl AsRef<Path>) -> Result<bool> {
    let a = a.as_ref();
    let b = b.as_ref();

    if !a.exists() {
        return Ok(false);
    }

    for a_sub in a.read_dir()? {
        let a_sub_path = a_sub?.path();
        let b_sub = change_base_path(&a_sub_path, a, b)?;
        if !b_sub.exists() {
            if a_sub_path.is_file() {
                return Ok(true);
            }

            if a_sub_path.is_dir() && !is_empty_dir(a_sub_path) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Change the path base to `new_base`
pub fn change_base_path(
    path: impl AsRef<Path>,
    base: impl AsRef<Path>,
    new_base: impl AsRef<Path>,
) -> Result<PathBuf> {
    Ok(new_base.as_ref().join(path.as_ref().strip_prefix(base)?))
}

/// find var and inplace
#[allow(clippy::string_slice)]
pub fn var_inplace<F>(
    content: &str,
    left_boundary: &str,
    right_boundary: &str,
    unwrap: bool,
    convert: F,
) -> Result<String>
where
    F: Fn(&str) -> Result<String>,
{
    let mut r = String::new();
    let mut last_index = 0;
    while let Some(li) = content[last_index..].find(left_boundary) {
        let content = &content[last_index..];
        r.push_str(&content[..li]);
        let content = &content[li..];
        if let Some(ri) = content.find(right_boundary)
            && !content[left_boundary.len()..ri].contains(left_boundary)
        {
            let dec_content = convert(&content[left_boundary.len()..ri])?;
            if !unwrap {
                r.push_str(left_boundary);
            }
            r.push_str(&dec_content);
            if !unwrap {
                r.push_str(right_boundary);
            }
            last_index = last_index + li + ri + right_boundary.len();
        } else {
            // 未闭合的分隔符，视为普通文本继续处理
            r.push_str(left_boundary);
            last_index = last_index + li + left_boundary.len();
        }
    }
    r.push_str(&content[last_index..]);
    Ok(r)
}

#[inline]
pub async fn canonicalize(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    futures::stream::iter(paths)
        .map(|path| async move {
            fs::canonicalize(&path)
                .await
                .with_context(|| format!("path: {}", path.display()))
        })
        .buffer_unordered(num_cpus::get())
        .try_collect()
        .await
}

#[inline]
pub fn hash(content: &str) -> String {
    let mut hasher = Sha3_256::new();
    hasher.update(content);
    let result = hasher.finalize();
    // format!("{result:x}")
    // result.iter().map(|b| format!("{:02x}", b)).collect::<String>()
    hex::encode(result)
}

#[cfg(test)]
mod test {

    use super::*;

    /// 未闭合的分隔符应视为普通文本，不会导致无限循环
    #[test]
    fn var_inplace_unclosed_left_boundary() {
        // 没有任何 "}" 的未闭合标记，&{ 应保留原样
        let content = "prefix &{no_close suffix";
        let r = var_inplace(
            content,
            "&{",
            "}",
            true,
            |s| Ok(s.to_uppercase()),
        )
        .unwrap();
        assert_eq!(r, "prefix &{no_close suffix");

        // 第一个 &{ 未闭合（其内容中包含另一个 &{），应视为普通文本
        let content = "a &{unclosed b &{inner} c";
        let r = var_inplace(
            content,
            "&{",
            "}",
            true,
            |s| Ok(s.to_uppercase()),
        )
        .unwrap();
        // 第一个 &{ 未闭合保留原样，&{inner} 正常处理为 INNER
        assert_eq!(r, "a &{unclosed b INNER c");
    }
}
