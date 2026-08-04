//! 虚拟文件树（Virtual File Tree）模块。
//!
//! 提供 `VNode` 数据结构用于在内存中表示文件系统层次结构，
//! 支持递归扫描、路径查找、节点移除等操作。
//! 该模块是虚拟树差异管线（diff pipeline）的基础。

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, DirEntry};
use std::io::ErrorKind;
use std::mem;
use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::error::Result;

/// 虚拟文件树节点类型。
#[derive(Debug, Clone)]
pub enum VNodeKind {
    /// 普通文件。
    File,
    /// 目录，包含子节点（完整递归扫描）。
    Dir,
    /// 浅层目录：文件系统上存在但未被递归展开（参照扫描中
    /// guide 不关心的目录），子树内容未知，视为非空叶子。
    ShallowDir,
    /// 符号链接，记录其指向的目标路径。
    Symlink {
        /// 符号链接指向的目标路径。
        target: PathBuf,
    },
}

/// 虚拟文件树节点。
///
/// 每个节点代表文件系统中的一个文件、目录或符号链接。
/// `rel_path` 仅保存当前节点自身的文件名（而非完整相对路径），
/// 层次关系通过 `children` 字段体现。
#[derive(Debug, Clone)]
pub struct VNode {
    /// 当前节点在父节点下的相对路径（仅为文件名）。
    pub rel_path: PathBuf,
    /// 文件系统上的绝对路径。
    pub abs_path: PathBuf,
    /// 节点类型。
    pub kind: VNodeKind,
    /// 子节点列表（仅对 Dir 类型有意义）。
    pub children: Vec<VNode>,
}

impl VNode {
    /// 扫描文件系统，从 `root` 开始构建虚拟文件树。
    ///
    /// 如果 `root` 路径不存在，返回一个空的 Dir 节点。
    /// 如果 `root` 存在，委托给 [`scan_recursive`] 进行递归扫描。
    pub fn scan(root: &Path, follow_symlinks: bool) -> Result<VNode> {
        match fs::symlink_metadata(root) {
            Ok(_) => scan_recursive(root, &PathBuf::new(), follow_symlinks),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(VNode {
                rel_path: PathBuf::new(),
                abs_path: root.to_path_buf(),
                kind: VNodeKind::Dir,
                children: Vec::new(),
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// 参照扫描目标目录。
    ///
    /// 以 `guide_tree` 的结构为指引，在 `target_root` 下扫描：
    /// - 每层做全量 `read_dir`（保证 fold 抑制和冲突检测的正确性）
    /// - 只递归展开 guide 中存在的子目录
    /// - guide 不关心的子目录标记为 [`VNodeKind::ShallowDir`]（不递归，视为非空）
    ///
    /// `guide_tree` 通常是 pack 目录的扫描结果（`VNode::scan(pack, false)`）。
    /// 当 target 是大目录（如 `~`）而 pack 很小时，此方法比 `scan` 快数个数量级。
    pub fn scan_guided(
        target_root: &Path,
        guide_tree: &VNode,
        follow_symlinks: bool,
    ) -> Result<VNode> {
        match fs::symlink_metadata(target_root) {
            Ok(_) => {
                scan_guided_recursive(target_root, &PathBuf::new(), guide_tree, follow_symlinks)
            }
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(VNode {
                rel_path: PathBuf::new(),
                abs_path: target_root.to_path_buf(),
                kind: VNodeKind::Dir,
                children: Vec::new(),
            }),
            Err(e) => Err(e.into()),
        }
    }

    /// 在树中查找与 `path` 匹配的节点（不可变引用）。
    ///
    /// 按路径组件逐层导航，通过匹配各节点的 `rel_path` 进行查找。
    /// `"."` 和空路径将返回 `self`。
    #[must_use]
    pub fn find(&self, path: &Path) -> Option<&VNode> {
        let mut current = self;
        for component in path.components() {
            let name = component.as_os_str();
            if name.is_empty() || name == "." {
                continue;
            }
            current = current
                .children
                .iter()
                .find(|c| c.rel_path.as_os_str() == name)?;
        }
        Some(current)
    }

    /// 在树中查找与 `path` 匹配的节点（可变引用）。
    ///
    /// 按路径组件逐层导航，通过匹配各节点的 `rel_path` 进行查找。
    /// `"."` 和空路径将返回 `self`。
    pub fn find_mut(&mut self, path: &Path) -> Option<&mut VNode> {
        let mut current = self;
        for component in path.components() {
            let name = component.as_os_str();
            if name.is_empty() || name == "." {
                continue;
            }
            let pos = current
                .children
                .iter()
                .position(|c| c.rel_path.as_os_str() == name)?;
            current = current.children.get_mut(pos)?;
        }
        Some(current)
    }

    /// 从树中移除与 `path` 匹配的节点并返回。
    ///
    /// - 单组件路径：直接查找并移除 `self.children` 中的匹配项。
    /// - 多组件路径：先通过 [`find_mut`] 定位父节点，再从父节点的 `children` 中移除。
    ///
    /// 返回被移除的节点；如果路径对应的节点不存在则返回 `None`。
    pub fn remove(&mut self, path: &Path) -> Option<VNode> {
        let components: Vec<_> = path
            .components()
            .filter(|c| {
                let s = c.as_os_str();
                !s.is_empty() && s != "."
            })
            .collect();

        if components.is_empty() {
            return None;
        }

        if components.len() == 1 {
            let target = components.first()?.as_os_str();
            if let Some(idx) = self
                .children
                .iter()
                .position(|c| c.rel_path.as_os_str() == target)
            {
                return Some(self.children.remove(idx));
            }
            return None;
        }

        // 多组件路径：定位父节点，然后从中移除目标子节点
        let target_name = components.last()?.as_os_str();

        let mut parent_path = PathBuf::new();
        for comp in components.iter().take(components.len() - 1) {
            parent_path.push(comp);
        }

        let parent = self.find_mut(&parent_path)?;
        if let Some(idx) = parent
            .children
            .iter()
            .position(|c| c.rel_path.as_os_str() == target_name)
        {
            return Some(parent.children.remove(idx));
        }

        None
    }

    /// 判断当前节点是否为叶子节点（`File`、`Symlink` 或 `ShallowDir`）。
    ///
    /// `ShallowDir` 没有展开的子节点，在树遍历中应作为叶子处理，
    /// 避免 `collect_empty_dirs` 等逻辑错误地递归进入。
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        matches!(
            self.kind,
            VNodeKind::File | VNodeKind::Symlink { .. } | VNodeKind::ShallowDir
        )
    }

    /// 判断当前节点是否为完整展开的目录（仅 `Dir` 变体）。
    ///
    /// `ShallowDir` 不被视为 `Dir` — 调用方不应尝试遍历其 `children`。
    #[must_use]
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, VNodeKind::Dir)
    }

    /// 从一组相对路径构建虚拟文件树（用作 `scan_guided` 的参照）。
    ///
    /// `root` 是树的根绝对路径，`leaf_paths` 是相对于 `root` 的文件路径列表。
    /// 中间目录自动创建为 `Dir` 节点，叶子节点为 `File` 节点。
    /// 如果 `leaf_paths` 为空，返回仅含根 `Dir` 的空树。
    #[must_use]
    pub fn from_paths(root: &Path, leaf_paths: &[PathBuf]) -> VNode {
        let mut root_node = new_dir(root);

        for leaf in leaf_paths {
            insert_leaf_path(&mut root_node, leaf);
        }

        root_node
    }

    /// 将树中的 `ShallowDir` 节点按 `guide` 树重新展开。
    ///
    /// 遍历 `self` 时，对每个 `ShallowDir`：
    /// - 若 `guide` 在对应路径有 `Dir` 节点 → 用完整扫描替换该 `ShallowDir`
    /// - 若 `guide` 没有对应节点 → 保持 `ShallowDir`
    ///
    /// `Dir` 节点会递归展开其子节点，`File`/`Symlink` 保持不变。
    pub fn expand_shallow(&mut self, guide: &VNode) -> Result<()> {
        let children = mem::take(&mut self.children);
        for mut child in children {
            if matches!(child.kind, VNodeKind::ShallowDir) {
                if let Some(guide_child) = guide
                    .children
                    .iter()
                    .find(|c| c.rel_path == child.rel_path && c.is_dir())
                {
                    child = scan_guided_recursive(
                        &child.abs_path,
                        &child.rel_path,
                        guide_child,
                        false,
                    )?;
                }
            } else if child.is_dir()
                && let Some(guide_child) = guide
                    .children
                    .iter()
                    .find(|c| c.rel_path == child.rel_path && c.is_dir())
            {
                child.expand_shallow(guide_child)?;
            }
            self.children.push(child);
        }
        self.children.sort_by_key(|c| c.rel_path.clone());
        Ok(())
    }
}

/// 创建根 `Dir` 节点（`rel_path` 为空，`abs_path` 指向 `root`）。
fn new_dir(root: &Path) -> VNode {
    VNode {
        rel_path: PathBuf::new(),
        abs_path: root.to_path_buf(),
        kind: VNodeKind::Dir,
        children: Vec::new(),
    }
}

/// 将一个相对路径插入到虚拟文件树中。
///
/// 按路径组件逐层创建中间 `Dir` 节点，将最后一个组件作为 `File` 节点插入。
/// 已存在的节点不会被覆盖或重复插入。
fn insert_leaf_path(parent: &mut VNode, rel_path: &Path) {
    let mut components = rel_path.components().peekable();
    let Some(first) = components.next() else {
        return;
    };

    let mut node: &mut VNode = parent;
    let mut comp = first;

    loop {
        let name = comp.as_os_str();
        let is_last = components.peek().is_none();
        let abs = node.abs_path.join(name);

        if is_last {
            // 叶子 File 节点：二分查找，不存在则插入到正确位置
            if let Err(idx) = node
                .children
                .binary_search_by(|c| c.rel_path.as_os_str().cmp(name))
            {
                node.children.insert(
                    idx,
                    VNode {
                        rel_path: PathBuf::from(name),
                        abs_path: abs,
                        kind: VNodeKind::File,
                        children: Vec::new(),
                    },
                );
            }
            return;
        }

        // 中间 Dir 节点：二分查找 — 找到即导航，未找到就 insert 并导航
        let idx = match node
            .children
            .binary_search_by(|c| c.rel_path.as_os_str().cmp(name))
        {
            Ok(idx) => idx,
            Err(idx) => {
                node.children.insert(
                    idx,
                    VNode {
                        rel_path: PathBuf::from(name),
                        abs_path: abs,
                        kind: VNodeKind::Dir,
                        children: Vec::new(),
                    },
                );
                idx
            }
        };
        // 两个分支中 idx 都保证有效，仅此处一个防御语句
        if let Some(child) = node.children.get_mut(idx) {
            node = child;
        } else {
            return;
        }

        let Some(next) = components.next() else {
            return;
        };
        comp = next;
    }
}

/// 递归扫描单个路径，构建 `VNode` 子树。
///
/// 使用 `fs::symlink_metadata` 而非 `fs::metadata`
/// 以避免 TOCTOU 竞态条件。符号链接在未启用 `follow_symlinks` 时
/// 不会被跟随。
fn scan_recursive(abs_path: &Path, rel_path: &Path, follow_symlinks: bool) -> Result<VNode> {
    let meta = fs::symlink_metadata(abs_path)
        .with_context(|| format!("Failed to read file metadata: {}", abs_path.display()))?;

    let ft = meta.file_type();

    // ── 符号链接处理 ──
    if ft.is_symlink() {
        if !follow_symlinks {
            let target = fs::read_link(abs_path)
                .with_context(|| format!("Failed to read symlink: {}", abs_path.display()))?;
            return Ok(VNode {
                rel_path: rel_path.to_path_buf(),
                abs_path: abs_path.to_path_buf(),
                kind: VNodeKind::Symlink { target },
                children: Vec::new(),
            });
        }

        // 跟随符号链接：使用 metadata 获取目标文件属性
        let resolved = fs::metadata(abs_path)
            .with_context(|| format!("Failed to resolve symlink target: {}", abs_path.display()))?;
        if resolved.is_dir() {
            return scan_dir_children(abs_path, rel_path, follow_symlinks);
        }
        // 目标不是目录，按普通文件处理
        return Ok(VNode {
            rel_path: rel_path.to_path_buf(),
            abs_path: abs_path.to_path_buf(),
            kind: VNodeKind::File,
            children: Vec::new(),
        });
    }

    // ── 目录处理 ──
    if ft.is_dir() {
        return scan_dir_children(abs_path, rel_path, follow_symlinks);
    }

    // ── 普通文件及未知类型（套接字、设备文件等）──
    Ok(VNode {
        rel_path: rel_path.to_path_buf(),
        abs_path: abs_path.to_path_buf(),
        kind: VNodeKind::File,
        children: Vec::new(),
    })
}

/// 扫描目录内的所有子条目，构建子节点列表。
///
/// 条目按文件名排序以确保确定性。
/// 对于每个子条目：如果 `follow_symlinks` 未启用且该条目是符号链接，
/// 则直接记录为 Symlink 节点（不递归）；否则递归扫描。
fn scan_dir_children(abs_path: &Path, rel_path: &Path, follow_symlinks: bool) -> Result<VNode> {
    let mut children = Vec::new();

    let mut entries: Vec<_> = fs::read_dir(abs_path)
        .with_context(|| format!("Failed to read directory: {}", abs_path.display()))?
        .filter_map(Result::ok)
        .collect();

    // 按文件名排序以保证确定性输出
    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        let child_abs = entry.path();
        let child_rel = entry.file_name();

        // 检查子条目是否为符号链接（不跟随的情况）
        if !follow_symlinks
            && let Ok(child_meta) = fs::symlink_metadata(&child_abs)
            && child_meta.file_type().is_symlink()
        {
            let target = fs::read_link(&child_abs).with_context(|| {
                format!("Failed to read child symlink: {}", child_abs.display())
            })?;
            children.push(VNode {
                rel_path: PathBuf::from(child_rel),
                abs_path: child_abs,
                kind: VNodeKind::Symlink { target },
                children: Vec::new(),
            });
            continue;
        }

        // 递归扫描子节点
        let child_node = scan_recursive(&child_abs, &PathBuf::from(child_rel), follow_symlinks)?;
        children.push(child_node);
    }

    Ok(VNode {
        rel_path: rel_path.to_path_buf(),
        abs_path: abs_path.to_path_buf(),
        kind: VNodeKind::Dir,
        children,
    })
}

/// 导览递归扫描 — 与 [`scan_recursive`] 行为相同，但目录子级使用
/// [`scan_guided_dir_children`] 替代 [`scan_dir_children`]。
fn scan_guided_recursive(
    abs_path: &Path,
    rel_path: &Path,
    guide_node: &VNode,
    follow_symlinks: bool,
) -> Result<VNode> {
    let meta = fs::symlink_metadata(abs_path)
        .with_context(|| format!("Failed to read file metadata: {}", abs_path.display()))?;

    let ft = meta.file_type();

    if ft.is_symlink() {
        if !follow_symlinks {
            let target = fs::read_link(abs_path)
                .with_context(|| format!("Failed to read symlink: {}", abs_path.display()))?;
            return Ok(VNode {
                rel_path: rel_path.to_path_buf(),
                abs_path: abs_path.to_path_buf(),
                kind: VNodeKind::Symlink { target },
                children: Vec::new(),
            });
        }

        let resolved = fs::metadata(abs_path)
            .with_context(|| format!("Failed to resolve symlink target: {}", abs_path.display()))?;
        if resolved.is_dir() {
            return scan_guided_dir_children(abs_path, rel_path, guide_node, follow_symlinks);
        }
        return Ok(VNode {
            rel_path: rel_path.to_path_buf(),
            abs_path: abs_path.to_path_buf(),
            kind: VNodeKind::File,
            children: Vec::new(),
        });
    }

    if ft.is_dir() {
        return scan_guided_dir_children(abs_path, rel_path, guide_node, follow_symlinks);
    }

    Ok(VNode {
        rel_path: rel_path.to_path_buf(),
        abs_path: abs_path.to_path_buf(),
        kind: VNodeKind::File,
        children: Vec::new(),
    })
}

/// 导览扫描目录子级 — 每层全量 `read_dir`，但只递归展开 guide 中存在的子目录。
/// guide 不关心的子目录标记为 `ShallowDir`。
fn scan_guided_dir_children(
    abs_path: &Path,
    rel_path: &Path,
    guide_node: &VNode,
    follow_symlinks: bool,
) -> Result<VNode> {
    // 从 guide 中提取关心的目录名集合（仅目录，文件/链接不影响递归决策）
    let guide_dirs: HashSet<OsString> = guide_node
        .children
        .iter()
        .filter(|c| c.is_dir())
        .map(|c| c.rel_path.as_os_str().to_os_string())
        .collect();

    let mut children = Vec::new();

    let mut entries: Vec<_> = fs::read_dir(abs_path)
        .with_context(|| format!("Failed to read directory: {}", abs_path.display()))?
        .filter_map(Result::ok)
        .collect();

    entries.sort_by_key(DirEntry::file_name);

    for entry in entries {
        let child_abs = entry.path();
        let child_rel = entry.file_name();

        // 符号链接处理（与 scan_dir_children 一致）
        if !follow_symlinks
            && let Ok(child_meta) = fs::symlink_metadata(&child_abs)
            && child_meta.file_type().is_symlink()
        {
            let target = fs::read_link(&child_abs).with_context(|| {
                format!("Failed to read child symlink: {}", child_abs.display())
            })?;
            children.push(VNode {
                rel_path: PathBuf::from(child_rel),
                abs_path: child_abs,
                kind: VNodeKind::Symlink { target },
                children: Vec::new(),
            });
            continue;
        }

        let child_meta = fs::symlink_metadata(&child_abs)?;

        if child_meta.is_dir() {
            if guide_dirs.contains(&child_rel) {
                // guide 关心的目录 → 递归展开
                if let Some(guide_child) = guide_node
                    .children
                    .iter()
                    .find(|c| c.rel_path.as_os_str() == child_rel && c.is_dir())
                {
                    let child_node = scan_guided_recursive(
                        &child_abs,
                        &PathBuf::from(child_rel),
                        guide_child,
                        follow_symlinks,
                    )?;
                    children.push(child_node);
                } else {
                    // guide_dirs 中标记为目录但在 guide_node.children 中未找到
                    // （防御性处理：guide 树状态不一致）
                    children.push(VNode {
                        rel_path: PathBuf::from(child_rel),
                        abs_path: child_abs,
                        kind: VNodeKind::ShallowDir,
                        children: Vec::new(),
                    });
                }
            } else {
                // guide 不关心的目录 → ShallowDir，不递归
                children.push(VNode {
                    rel_path: PathBuf::from(child_rel),
                    abs_path: child_abs,
                    kind: VNodeKind::ShallowDir,
                    children: Vec::new(),
                });
            }
        } else {
            // 普通文件 / 设备文件等 → 正常叶子节点
            children.push(VNode {
                rel_path: PathBuf::from(child_rel),
                abs_path: child_abs,
                kind: VNodeKind::File,
                children: Vec::new(),
            });
        }
    }

    Ok(VNode {
        rel_path: rel_path.to_path_buf(),
        abs_path: abs_path.to_path_buf(),
        kind: VNodeKind::Dir,
        children,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// 在临时目录中创建测试文件结构，返回临时目录路径。
    /// 使用原子计数器为每个测试分配唯一子目录，避免并行测试冲突。
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("vtree_test_{}_{}", std::process::id(), id));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_empty_dir() {
        let dir = temp_dir();
        let tree = VNode::scan(&dir, false).unwrap();
        assert!(tree.is_dir());
        assert!(tree.children.is_empty());
        assert_eq!(tree.rel_path, PathBuf::new());
    }

    #[test]
    fn scan_single_file() {
        let dir = temp_dir();
        let file_path = dir.join("hello.txt");
        std::fs::write(&file_path, "content").unwrap();

        let tree = VNode::scan(&dir, false).unwrap();
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].rel_path, PathBuf::from("hello.txt"));
        assert!(matches!(tree.children[0].kind, VNodeKind::File));
    }

    #[test]
    fn scan_nested() {
        let dir = temp_dir();
        let sub_dir = dir.join("subdir");
        std::fs::create_dir(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("file.txt"), "nested content").unwrap();

        let tree = VNode::scan(&dir, false).unwrap();
        assert_eq!(tree.children.len(), 1);

        let subdir_node = &tree.children[0];
        assert_eq!(subdir_node.rel_path, PathBuf::from("subdir"));
        assert!(subdir_node.is_dir());
        assert_eq!(subdir_node.children.len(), 1);
        assert_eq!(subdir_node.children[0].rel_path, PathBuf::from("file.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn scan_symlink() {
        let dir = temp_dir();
        let target_file = dir.join("target.txt");
        std::fs::write(&target_file, "target content").unwrap();
        let link = dir.join("link.txt");
        std::os::unix::fs::symlink(&target_file, &link).unwrap();

        let tree = VNode::scan(&dir, false).unwrap();
        assert_eq!(tree.children.len(), 2); // target.txt + link.txt

        let link_node = tree
            .children
            .iter()
            .find(|c| c.rel_path == PathBuf::from("link.txt"))
            .unwrap();
        assert!(matches!(link_node.kind, VNodeKind::Symlink { .. }));
        if let VNodeKind::Symlink { ref target } = link_node.kind {
            assert_eq!(*target, target_file);
        }
    }

    #[cfg(unix)]
    #[test]
    fn scan_symlink_follow() {
        let dir = temp_dir();
        let sub_dir = dir.join("realdir");
        std::fs::create_dir(&sub_dir).unwrap();
        std::fs::write(sub_dir.join("inner.txt"), "inner").unwrap();
        let link = dir.join("linkdir");
        std::os::unix::fs::symlink(&sub_dir, &link).unwrap();

        let tree = VNode::scan(&dir, true).unwrap();
        assert_eq!(tree.children.len(), 2);

        let linked_node = tree
            .children
            .iter()
            .find(|c| c.rel_path == PathBuf::from("linkdir"))
            .unwrap();
        assert!(linked_node.is_dir());
        assert_eq!(linked_node.children.len(), 1);
        assert_eq!(linked_node.children[0].rel_path, PathBuf::from("inner.txt"));
    }

    #[test]
    fn find_existing() {
        let dir = temp_dir();
        let sub = dir.join("a");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("b.txt"), "b").unwrap();

        let tree = VNode::scan(&dir, false).unwrap();

        let found = tree.find(Path::new("a")).unwrap();
        assert!(found.is_dir());
        assert_eq!(found.rel_path, PathBuf::from("a"));

        let found_deep = tree.find(Path::new("a/b.txt")).unwrap();
        assert!(matches!(found_deep.kind, VNodeKind::File));
        assert_eq!(found_deep.rel_path, PathBuf::from("b.txt"));

        // "." 返回自身
        let root = tree.find(Path::new(".")).unwrap();
        assert!(root.is_dir());
        assert_eq!(root.rel_path, PathBuf::new());
    }

    #[test]
    fn find_nonexistent() {
        let dir = temp_dir();
        let tree = VNode::scan(&dir, false).unwrap();
        assert!(tree.find(Path::new("nope")).is_none());
        assert!(tree.find(Path::new("a/b/c")).is_none());
    }

    #[test]
    fn remove_single_component() {
        let dir = temp_dir();
        std::fs::write(dir.join("x.txt"), "x").unwrap();
        std::fs::write(dir.join("y.txt"), "y").unwrap();

        let mut tree = VNode::scan(&dir, false).unwrap();
        assert_eq!(tree.children.len(), 2);

        let removed = tree.remove(Path::new("x.txt")).unwrap();
        assert_eq!(removed.rel_path, PathBuf::from("x.txt"));
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].rel_path, PathBuf::from("y.txt"));
    }

    #[test]
    fn remove_nested() {
        let dir = temp_dir();
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("f.txt"), "f").unwrap();
        std::fs::write(sub.join("g.txt"), "g").unwrap();

        let mut tree = VNode::scan(&dir, false).unwrap();
        assert_eq!(tree.children.len(), 1);

        let removed = tree.remove(Path::new("sub/f.txt")).unwrap();
        assert_eq!(removed.rel_path, PathBuf::from("f.txt"));
        assert!(matches!(removed.kind, VNodeKind::File));

        // 验证 g.txt 仍然存在
        let sub_node = tree.find(Path::new("sub")).unwrap();
        assert_eq!(sub_node.children.len(), 1);
        assert_eq!(sub_node.children[0].rel_path, PathBuf::from("g.txt"));
    }

    #[test]
    fn remove_nonexistent() {
        let dir = temp_dir();
        let mut tree = VNode::scan(&dir, false).unwrap();
        assert!(tree.remove(Path::new("nothing")).is_none());
    }

    #[test]
    fn is_leaf_checks() {
        let file_node = VNode {
            rel_path: PathBuf::from("f.txt"),
            abs_path: PathBuf::from("/tmp/f.txt"),
            kind: VNodeKind::File,
            children: Vec::new(),
        };
        assert!(file_node.is_leaf());
        assert!(!file_node.is_dir());

        let symlink_node = VNode {
            rel_path: PathBuf::from("s"),
            abs_path: PathBuf::from("/tmp/s"),
            kind: VNodeKind::Symlink {
                target: PathBuf::from("/tmp/target"),
            },
            children: Vec::new(),
        };
        assert!(symlink_node.is_leaf());
        assert!(!symlink_node.is_dir());

        let dir_node = VNode {
            rel_path: PathBuf::new(),
            abs_path: PathBuf::from("/tmp/d"),
            kind: VNodeKind::Dir,
            children: Vec::new(),
        };
        assert!(!dir_node.is_leaf());
        assert!(dir_node.is_dir());

        let shallow_node = VNode {
            rel_path: PathBuf::from("shallow"),
            abs_path: PathBuf::from("/tmp/shallow"),
            kind: VNodeKind::ShallowDir,
            children: Vec::new(),
        };
        assert!(shallow_node.is_leaf());
        assert!(!shallow_node.is_dir());
    }

    #[test]
    fn scan_nonexistent_root() {
        let nonexistent = PathBuf::from("/nonexistent/path/for/test");
        let tree = VNode::scan(&nonexistent, false).unwrap();
        assert!(tree.is_dir());
        assert!(tree.children.is_empty());
        assert_eq!(tree.abs_path, nonexistent);
        assert_eq!(tree.rel_path, PathBuf::new());
    }

    // ── scan_guided 测试 ──

    /// 在临时目录中创建双树结构：pack 树（小）和 target 树（大，含无关目录）。
    /// 返回 (target_root, pack_root)。
    fn temp_twin_trees() -> (PathBuf, PathBuf) {
        let target = temp_dir();
        let pack = temp_dir();

        // pack 树: nvim/
        let pack_nvim = pack.join("nvim");
        std::fs::create_dir(&pack_nvim).unwrap();
        std::fs::write(pack_nvim.join("init.lua"), "pack init").unwrap();
        let pack_lua = pack_nvim.join("lua");
        std::fs::create_dir(&pack_lua).unwrap();
        std::fs::write(pack_lua.join("plugins.lua"), "pack plugins").unwrap();

        // target 树: nvim/ (对应 pack) + fish/ (无关) + git/ (无关)
        let target_nvim = target.join("nvim");
        std::fs::create_dir(&target_nvim).unwrap();
        std::fs::write(target_nvim.join("init.lua"), "target init").unwrap();
        let target_lua = target_nvim.join("lua");
        std::fs::create_dir(&target_lua).unwrap();
        std::fs::write(target_lua.join("plugins.lua"), "target plugins").unwrap();
        // nvim 下还有一个 pack 中没有的文件（测试 fold 抑制）
        std::fs::write(target_nvim.join("custom.vim"), "custom").unwrap();

        // 无关目录（不应被递归扫描）
        let target_fish = target.join("fish");
        std::fs::create_dir(&target_fish).unwrap();
        std::fs::write(target_fish.join("config.fish"), "fish config").unwrap();
        let fish_sub = target_fish.join("completions");
        std::fs::create_dir(&fish_sub).unwrap();
        std::fs::write(fish_sub.join("git.fish"), "fish completion").unwrap();

        let target_git = target.join("git");
        std::fs::create_dir(&target_git).unwrap();
        std::fs::write(target_git.join("config"), "git config").unwrap();

        (target, pack)
    }

    #[test]
    fn scan_guided_marks_unrelated_as_shallow() {
        let (target, pack) = temp_twin_trees();
        let guide_tree = VNode::scan(&pack, false).unwrap();

        // pack 中包含 nvim，不包含 fish 和 git
        let target_tree = VNode::scan_guided(&target, &guide_tree, false).unwrap();

        assert!(target_tree.is_dir());
        assert_eq!(target_tree.children.len(), 3);

        // nvim → 应被完整展开（在 guide 中）
        let nvim = target_tree.find(Path::new("nvim")).unwrap();
        assert!(nvim.is_dir());
        assert!(!nvim.is_leaf());
        // nvim 内部应完整展开（含 custom.vim）
        assert_eq!(nvim.children.len(), 3); // init.lua, lua/, custom.vim

        // fish → ShallowDir（不在 guide 中）
        let fish = target_tree.find(Path::new("fish")).unwrap();
        assert!(matches!(fish.kind, VNodeKind::ShallowDir));
        assert!(fish.is_leaf());
        assert!(!fish.is_dir());
        assert!(fish.children.is_empty());

        // git → ShallowDir（不在 guide 中）
        let git = target_tree.find(Path::new("git")).unwrap();
        assert!(matches!(git.kind, VNodeKind::ShallowDir));
        assert!(git.is_leaf());
    }

    #[test]
    fn scan_guided_fully_expands_guide_dirs() {
        let (target, pack) = temp_twin_trees();
        let guide_tree = VNode::scan(&pack, false).unwrap();
        let target_tree = VNode::scan_guided(&target, &guide_tree, false).unwrap();

        // nvim/lua/plugins.lua 应在 target 树中（guide 命中 → 全展开）
        let plugins = target_tree.find(Path::new("nvim/lua/plugins.lua")).unwrap();
        assert!(matches!(plugins.kind, VNodeKind::File));
        assert_eq!(plugins.abs_path, target.join("nvim/lua/plugins.lua"));
    }

    #[test]
    fn scan_guided_no_pack_path_in_target() {
        let target = temp_dir();
        let pack = temp_dir();

        // pack 有 sub/，但 target 没有
        std::fs::create_dir(pack.join("sub")).unwrap();
        std::fs::write(pack.join("sub").join("a.txt"), "a").unwrap();

        // target 有其他内容
        std::fs::create_dir(target.join("other")).unwrap();
        std::fs::write(target.join("other").join("b.txt"), "b").unwrap();

        let guide_tree = VNode::scan(&pack, false).unwrap();
        let target_tree = VNode::scan_guided(&target, &guide_tree, false).unwrap();

        // other 是 ShallowDir
        let other = target_tree.find(Path::new("other")).unwrap();
        assert!(matches!(other.kind, VNodeKind::ShallowDir));

        // sub 不在 target 中—不应出现
        assert!(target_tree.find(Path::new("sub")).is_none());
    }

    #[test]
    fn scan_guided_nonexistent_target() {
        let pack = temp_dir();
        std::fs::create_dir(pack.join("nvim")).unwrap();
        std::fs::write(pack.join("nvim").join("init.lua"), "init").unwrap();

        let nonexistent = PathBuf::from("/nonexistent/target/for/guided_test");
        let guide_tree = VNode::scan(&pack, false).unwrap();
        let tree = VNode::scan_guided(&nonexistent, &guide_tree, false).unwrap();

        assert!(tree.is_dir());
        assert!(tree.children.is_empty());
        assert_eq!(tree.abs_path, nonexistent);
    }

    #[test]
    fn scan_guided_symlink_handling() {
        let target = temp_dir();
        let pack = temp_dir();

        // pack 里有 pack_file.txt
        std::fs::write(pack.join("pack_file.txt"), "pack").unwrap();

        // target 里有同名文件和一个符号链接
        std::fs::write(target.join("pack_file.txt"), "target").unwrap();
        let link_dst = target.join("my_link");
        std::os::unix::fs::symlink("/some/where", &link_dst).unwrap();

        let guide_tree = VNode::scan(&pack, false).unwrap();
        let target_tree = VNode::scan_guided(&target, &guide_tree, false).unwrap();

        // pack_file.txt → File（guide 中存在同名 → 但仍然按文件处理）
        let f = target_tree.find(Path::new("pack_file.txt")).unwrap();
        assert!(matches!(f.kind, VNodeKind::File));

        // my_link → Symlink（不在 guide 中，按正常符号链接处理）
        let l = target_tree.find(Path::new("my_link")).unwrap();
        assert!(matches!(l.kind, VNodeKind::Symlink { .. }));
    }

    // ── from_paths 测试 ──

    #[test]
    fn from_paths_empty() {
        let tree = VNode::from_paths(Path::new("/root"), &[]);
        assert!(tree.is_dir());
        assert!(tree.children.is_empty());
        assert_eq!(tree.abs_path, Path::new("/root"));
        assert_eq!(tree.rel_path, PathBuf::new());
    }

    #[test]
    fn from_paths_single_file() {
        let paths = [PathBuf::from("a.txt")];
        let tree = VNode::from_paths(Path::new("/root"), &paths);
        assert_eq!(tree.children.len(), 1);
        assert!(matches!(tree.children[0].kind, VNodeKind::File));
        assert_eq!(tree.children[0].rel_path, PathBuf::from("a.txt"));
        assert_eq!(tree.children[0].abs_path, Path::new("/root/a.txt"));
    }

    #[test]
    fn from_paths_nested() {
        let paths = [
            PathBuf::from("a/b/c.txt"),
            PathBuf::from("a/b/d.txt"),
            PathBuf::from("a/x.txt"),
        ];
        let tree = VNode::from_paths(Path::new("/root"), &paths);
        assert_eq!(tree.children.len(), 1);

        let a = &tree.children[0];
        assert_eq!(a.rel_path, PathBuf::from("a"));
        assert!(a.is_dir());
        assert_eq!(a.children.len(), 2); // b, x.txt

        // x.txt
        let x = a
            .children
            .iter()
            .find(|c| c.rel_path == PathBuf::from("x.txt"))
            .unwrap();
        assert!(matches!(x.kind, VNodeKind::File));

        // b/
        let b = a
            .children
            .iter()
            .find(|c| c.rel_path == PathBuf::from("b"))
            .unwrap();
        assert!(b.is_dir());
        assert_eq!(b.children.len(), 2); // c.txt, d.txt
    }

    #[test]
    fn from_paths_duplicates() {
        let paths = [PathBuf::from("a.txt"), PathBuf::from("a.txt")];
        let tree = VNode::from_paths(Path::new("/root"), &paths);
        assert_eq!(tree.children.len(), 1);
    }

    // ── expand_shallow 测试 ──

    /// 创建 guide 树和目标树（含 ShallowDir），测试 expand_shallow。
    fn setup_expand_test() -> (PathBuf, VNode, VNode) {
        let target = temp_dir();
        let pack = temp_dir();

        // pack: nvim/init.lua
        std::fs::create_dir(pack.join("nvim")).unwrap();
        std::fs::write(pack.join("nvim").join("init.lua"), "pack init").unwrap();

        // target: nvim/ (含 init.lua + custom.vim) + fish/
        std::fs::create_dir(target.join("nvim")).unwrap();
        std::fs::write(target.join("nvim").join("init.lua"), "target init").unwrap();
        std::fs::write(target.join("nvim").join("custom.vim"), "custom").unwrap();
        std::fs::create_dir(target.join("fish")).unwrap();
        std::fs::write(target.join("fish").join("config.fish"), "fish").unwrap();

        let pack_tree = VNode::scan(&pack, false).unwrap();
        let target_tree = VNode::scan_guided(&target, &pack_tree, false).unwrap();

        (target, pack_tree, target_tree)
    }

    #[test]
    fn expand_shallow_expands_guide_matching_dir() {
        let (_target, _pack_tree, mut target_tree) = setup_expand_test();

        // nvim 在 guide 中 → 已全展开，不是 ShallowDir
        let nvim = target_tree.find(Path::new("nvim")).unwrap();
        assert!(nvim.is_dir());

        // fish 不在 guide 中 → ShallowDir
        let fish = target_tree.find(Path::new("fish")).unwrap();
        assert!(matches!(fish.kind, VNodeKind::ShallowDir));

        // 构造一个也包含 fish/ 目录的 guide（用实际目录扫描）
        let guide_dir = temp_dir();
        std::fs::create_dir(guide_dir.join("fish")).unwrap();
        std::fs::write(guide_dir.join("fish").join("config.fish"), "guide fish").unwrap();
        std::fs::create_dir(guide_dir.join("nvim")).unwrap();
        let expanded_guide = VNode::scan(&guide_dir, false).unwrap();

        target_tree.expand_shallow(&expanded_guide).unwrap();

        // fish 应被展开为完整 Dir
        let fish = target_tree.find(Path::new("fish")).unwrap();
        assert!(fish.is_dir());
        assert!(!fish.is_leaf());
        assert_eq!(fish.children.len(), 1); // config.fish
        assert!(matches!(fish.children[0].kind, VNodeKind::File));
    }

    #[test]
    fn expand_shallow_ignores_non_matching_guide() {
        let (_target, _pack_tree, mut target_tree) = setup_expand_test();

        // expand 一个空的 guide：什么都不应改变
        let empty_guide = VNode::from_paths(Path::new("/nonexistent"), &[]);

        target_tree.expand_shallow(&empty_guide).unwrap();

        let fish = target_tree.find(Path::new("fish")).unwrap();
        assert!(matches!(fish.kind, VNodeKind::ShallowDir));
    }
}
