//! 虚拟文件树（Virtual File Tree）模块。
//!
//! 提供 `VNode` 数据结构用于在内存中表示文件系统层次结构，
//! 支持递归扫描、路径查找、节点移除等操作。
//! 该模块是虚拟树差异管线（diff pipeline）的基础。

use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::error::Result;

/// 虚拟文件树节点类型。
#[derive(Debug, Clone)]
pub enum VNodeKind {
    /// 普通文件。
    File,
    /// 目录，包含子节点。
    Dir,
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
        match std::fs::symlink_metadata(root) {
            Ok(_) => scan_recursive(root, &PathBuf::new(), follow_symlinks),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(VNode {
                rel_path: PathBuf::new(),
                abs_path: root.to_path_buf(),
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

    /// 判断当前节点是否为叶子节点（File 或 Symlink）。
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        matches!(self.kind, VNodeKind::File | VNodeKind::Symlink { .. })
    }

    /// 判断当前节点是否为目录。
    #[must_use]
    pub fn is_dir(&self) -> bool {
        matches!(self.kind, VNodeKind::Dir)
    }
}

/// 递归扫描单个路径，构建 `VNode` 子树。
///
/// 使用 `std::fs::symlink_metadata` 而非 `std::fs::metadata`
/// 以避免 TOCTOU 竞态条件。符号链接在未启用 `follow_symlinks` 时
/// 不会被跟随。
fn scan_recursive(abs_path: &Path, rel_path: &Path, follow_symlinks: bool) -> Result<VNode> {
    let meta = std::fs::symlink_metadata(abs_path)
        .with_context(|| format!("Failed to read file metadata: {}", abs_path.display()))?;

    let ft = meta.file_type();

    // ── 符号链接处理 ──
    if ft.is_symlink() {
        if !follow_symlinks {
            let target = std::fs::read_link(abs_path)
                .with_context(|| format!("Failed to read symlink: {}", abs_path.display()))?;
            return Ok(VNode {
                rel_path: rel_path.to_path_buf(),
                abs_path: abs_path.to_path_buf(),
                kind: VNodeKind::Symlink { target },
                children: Vec::new(),
            });
        }

        // 跟随符号链接：使用 metadata 获取目标文件属性
        let resolved = std::fs::metadata(abs_path)
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

    let mut entries: Vec<_> = std::fs::read_dir(abs_path)
        .with_context(|| format!("Failed to read directory: {}", abs_path.display()))?
        .filter_map(std::result::Result::ok)
        .collect();

    // 按文件名排序以保证确定性输出
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let child_abs = entry.path();
        let child_rel = entry.file_name();

        // 检查子条目是否为符号链接（不跟随的情况）
        if !follow_symlinks
            && let Ok(child_meta) = std::fs::symlink_metadata(&child_abs)
            && child_meta.file_type().is_symlink()
        {
            let target = std::fs::read_link(&child_abs).with_context(|| {
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
}
