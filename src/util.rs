use std::borrow::Cow;
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

/// 扫描字符串中的占位符 `left...right`，对匹配的占位符内容调用 `convert` 进行原地替换。
///
/// - `unwrap`: true 表示移除分隔符，只保留替换结果；false 保留分隔符包裹替换结果。
/// - 嵌套处理：如果当前 `left` 和对应的 `right` 之间出现了新的 `left`，则当前占位符被视为未闭合，
///   其 `left` 作为普通文本保留，让内层的 `left...right` 成为新的匹配。
/// - 快速路径：如果字符串中不存在 `left` 分隔符，直接返回 `Cow::Borrowed`，零分配。
///
/// 采用位置跟踪式循环，在有效占位符分支中利用已扫描到的 `next_left` 直接跳转，
/// 避免下一轮循环重复扫描已知位置的 `left`。
#[allow(clippy::string_slice)]
pub fn var_inplace<'a, F>(
    content: &'a str,
    left: &str,
    right: &str,
    unwrap: bool,
    convert: F,
) -> Result<Cow<'a, str>>
where
    F: Fn(&str) -> Result<String>,
{
    if !content.contains(left) {
        return Ok(Cow::Borrowed(content));
    }

    let mut result = String::with_capacity(content.len());
    let mut pos = 0;
    let mut next_left: Option<usize> = content[pos..].find(left).map(|o| pos + o);

    while let Some(abs_left) = next_left {
        result.push_str(&content[pos..abs_left]);

        let after_left = abs_left + left.len();
        let rest = &content[after_left..];
        let next_right = rest.find(right);
        let next_left_rel = rest.find(left);
        next_left = next_left_rel.map(|li| after_left + li);

        // rest 中无 right → 当前 left 及后续所有 left 均无法闭合
        let Some(ri) = next_right else {
            result.push_str(&content[abs_left..]);
            return Ok(Cow::Owned(result));
        };

        if next_left_rel.is_none_or(|li| ri < li) {
            // Case 1: 有效占位符 — right 在 next_left 之前（或无 next_left）
            let inner = &rest[..ri];
            let replaced = convert(inner)?;
            if !unwrap {
                result.push_str(left);
            }
            result.push_str(&replaced);
            if !unwrap {
                result.push_str(right);
            }

            if let Some(nl) = next_left {
                let after_right = after_left + ri + right.len();
                debug_assert!(after_right <= nl, "left 与 right 重叠，跳转优化不适用");
                result.push_str(&content[after_right..nl]);
                pos = nl;
            } else {
                result.push_str(&content[after_left + ri + right.len()..]);
                return Ok(Cow::Owned(result));
            }
        } else if let Some(li) = next_left_rel {
            // Case 2: 嵌套 — 内层 left 先于外层 right 出现 (ri >= li)
            let nl = after_left + li;
            result.push_str(&content[abs_left..nl]);
            pos = nl;
        }
    }

    result.push_str(&content[pos..]);
    Ok(Cow::Owned(result))
}

/// 从路径中提取包名（最后一级目录名）
#[inline]
pub fn pack_name(pack: &Path) -> Result<&str> {
    pack.file_name()
        .and_then(|it| it.to_str())
        .ok_or_else(|| anyhow!("path error: {}", pack.display()))
}

/// 计算异步文件操作流的最佳并发上限
#[inline]
pub fn max_concurrent_files() -> usize {
    num_cpus::get() * 4
}

/// Pack 级并发上限（小于文件级，因为每个 pack 内部还会展开文件级并发）
#[inline]
pub fn max_concurrent_packs() -> usize {
    num_cpus::get() * 2
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
mod tests {
    use super::*;

    mod var_inplace {
        use super::*;

        #[test]
        fn nested_inner_wins() {
            assert_eq!(
                var_inplace("a &{outer &{inner} c", "&{", "}", true, |s| Ok(
                    s.to_uppercase()
                ))
                .unwrap(),
                "a &{outer INNER c"
            );
        }

        #[test]
        fn unclosed_left_kept_literal() {
            assert_eq!(
                var_inplace("prefix &{no_close suffix", "&{", "}", true, |s| Ok(
                    s.to_uppercase()
                ))
                .unwrap(),
                "prefix &{no_close suffix"
            );
        }

        #[test]
        fn empty_inner() {
            assert_eq!(
                var_inplace("&{}", "&{", "}", true, |_s| Ok("replaced".to_owned())).unwrap(),
                "replaced"
            );
        }

        #[test]
        fn no_markers_unchanged() {
            assert_eq!(
                var_inplace("plain text", "&{", "}", true, |_s| unreachable!()).unwrap(),
                "plain text"
            );
        }

        #[test]
        fn error_propagation() {
            let r = var_inplace("&{x}", "&{", "}", true, |_s| Err(anyhow::anyhow!("boom")));
            assert!(r.is_err());
        }

        #[test]
        fn triple_open_brace_delimiter() {
            assert_eq!(
                var_inplace("${{{123}}}", "${{{", "}}", true, |s: &str| Ok(
                    s.to_uppercase()
                ))
                .unwrap(),
                "123}"
            );
        }

        #[test]
        fn large_text() {
            let content = (0..1000)
                .map(|i| format!("&{{item{i}}}"))
                .collect::<String>();
            let r = var_inplace(&content, "&{", "}", true, |s| Ok(s.to_uppercase())).unwrap();
            for i in 0..1000 {
                assert!(r.contains(&format!("ITEM{i}")));
            }
            assert!(!r.contains("&{") && !r.contains("}"));
        }

        #[test]
        fn unclosed_then_valid() {
            assert_eq!(
                var_inplace("&{a &{b} &{c}", "&{", "}", true, |s| Ok(s.to_uppercase())).unwrap(),
                "&{a B C"
            );
        }

        #[test]
        fn keep_delimiters_with_unclosed() {
            assert_eq!(
                var_inplace("&{a &{b}", "&{", "}", false, |s| Ok(s.to_uppercase())).unwrap(),
                "&{a &{B}"
            );
        }
    }
}
