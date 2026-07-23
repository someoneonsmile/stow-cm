//! 集成测试共享基础设施。
//!
//! 提供：
//! - [`TestEnv`] — 隔离的测试环境（独立 XDG 目录）
//! - Pack 构建辅助函数
//! - 断言辅助函数（symlink 验证、track file 验证等）

#![allow(clippy::indexing_slicing)]
#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use stow_cm::config::Config;
use tempfile::TempDir;

// ── 环境变量序列化锁 ──
// XDG_*_HOME 是进程级环境变量，并行测试不能同时修改它们。
// 用 Mutex 保证同一时间只有一个 TestEnv 处于 active 状态。
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// 隔离的测试环境。
///
/// 持有 XDG 环境变量锁，设置 `XDG_CONFIG_HOME` 和 `XDG_STATE_HOME` 到临时目录。
/// Drop 时自动清理临时目录（通过 `TempDir`）。
pub struct TestEnv {
    _guard: std::sync::MutexGuard<'static, ()>,
    /// 用于 `XDG_CONFIG_HOME` 的临时目录
    config_dir: TempDir,
    /// 用于 `XDG_STATE_HOME` 的临时目录
    state_dir: TempDir,
}

impl TestEnv {
    /// 创建新的隔离测试环境。
    ///
    /// 会设置 `XDG_CONFIG_HOME` 和 `XDG_STATE_HOME` 环境变量到临时目录。
    /// 由于持有全局锁，**同一时间只能存在一个 `TestEnv` 实例**。
    pub fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        let config_dir =
            TempDir::with_prefix("stow-cm-test-config-").expect("failed to create config temp dir");
        let state_dir =
            TempDir::with_prefix("stow-cm-test-state-").expect("failed to create state temp dir");

        // SAFETY: set_var is marked unsafe in Rust 2024 because concurrent
        // access can cause UB. We hold ENV_LOCK which serializes all TestEnv
        // creation, so no concurrent access is possible.
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", config_dir.path());
            std::env::set_var("XDG_STATE_HOME", state_dir.path());
        }

        TestEnv {
            _guard: guard,
            config_dir,
            state_dir,
        }
    }

    /// 获取 config 目录路径
    pub fn config_path(&self) -> &Path {
        self.config_dir.path()
    }

    /// 获取 state 目录路径
    pub fn state_path(&self) -> &Path {
        self.state_dir.path()
    }
}

// ── Pack 辅助函数 ──

/// 在 `base_dir` 下创建 pack 目录，写入 `stow-cm.toml` 和测试文件。
///
/// 返回完整的 pack 目录路径。pack 目录会随 `TestEnv` 的 drop 自动清理。
pub fn create_pack(base_dir: &Path, name: &str, config_toml: &str) -> PathBuf {
    let pack_dir = base_dir.join(name);
    std::fs::create_dir_all(&pack_dir).expect("failed to create pack dir");

    let config_path = pack_dir.join("stow-cm.toml");
    std::fs::write(&config_path, config_toml).expect("failed to write stow-cm.toml");

    pack_dir
}

/// 在 pack 目录中创建文件。会自动创建父目录。
pub fn write_pack_file(
    pack_dir: &Path,
    relative_path: impl AsRef<Path>,
    content: impl AsRef<[u8]>,
) {
    let full_path = pack_dir.join(relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("failed to create parent dir");
    }
    std::fs::write(&full_path, content).expect("failed to write file");
}

// ── 配置构建器 ──

/// 构建全局 `Config`。
pub fn make_global_config() -> Config {
    Config::global().expect("failed to create global config")
}

/// 构建 pack 配置字符串（symlink 模式）。
pub fn pack_config(target: &str) -> String {
    format!(
        r#"target = '{target}'
mode = 'symlink'
"#
    )
}

/// 构建带 copy 模式的 pack 配置字符串。
pub fn pack_config_copy(target: &str) -> String {
    format!(
        r#"target = '{target}'
mode = 'copy'
"#
    )
}

/// 构建带 ignore 的 pack 配置字符串。
pub fn pack_config_with_ignore(target: &str, ignore_patterns: &[&str]) -> String {
    let ignore_str = ignore_patterns
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"target = '{target}'
mode = 'symlink'
ignore = [{ignore_str}]
"#
    )
}

/// 构建带 override 的 pack 配置字符串。
pub fn pack_config_with_override(target: &str, over_patterns: &[&str]) -> String {
    let over_str = over_patterns
        .iter()
        .map(|p| format!("'{p}'"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"target = '{target}'
mode = 'symlink'
override = [{over_str}]
"#
    )
}

/// 构建带加密的 pack 配置字符串。
pub fn pack_config_encrypted(target: &str, key_path: &str) -> String {
    format!(
        r#"target = '{target}'
mode = 'symlink'

[encrypted]
enable = true
key_path = '{key_path}'
left_boundary = '&{{'
right_boundary = '}}'
encrypted_alg = 'ChaCha20-Poly1305'
decrypted_path = '${{XDG_STATE_HOME:-~/.local/state}}/stow-cm/${{PACK_ID}}/decrypted/'
"#
    )
}

// ── 加密密钥辅助 ──

/// 创建测试用的加密密钥文件，返回密钥文件路径和 key bytes。
pub fn create_test_key(env: &TestEnv) -> (PathBuf, Vec<u8>) {
    // 256-bit 固定测试密钥 (AES-256-GCM / ChaCha20-Poly1305)
    let key = b"0123456789abcdef0123456789abcdef";
    let key_base64 = stow_cm::base64::encode(key);
    let key_path = env.state_path().join("test-key");
    std::fs::write(&key_path, &key_base64).expect("failed to write key file");
    (key_path, key.to_vec())
}

// ── 断言辅助 ──

/// 断言指定路径是一个指向预期目标的 symlink。
pub fn assert_symlink(link_path: impl AsRef<Path>, expected_target: impl AsRef<Path>) {
    let link_path = link_path.as_ref();
    let metadata = std::fs::symlink_metadata(link_path)
        .unwrap_or_else(|_| panic!("symlink_metadata failed for {}", link_path.display()));
    assert!(
        metadata.file_type().is_symlink(),
        "expected symlink at {}, got {:?}",
        link_path.display(),
        metadata.file_type()
    );
    let actual_target = std::fs::read_link(link_path)
        .unwrap_or_else(|_| panic!("read_link failed for {}", link_path.display()));
    assert_eq!(
        actual_target,
        expected_target.as_ref(),
        "symlink target mismatch at {}",
        link_path.display()
    );
}

/// 断言指定路径是一个普通文件，且内容与源文件一致（copy 模式）。
pub fn assert_copy(link_path: impl AsRef<Path>, expected_src: impl AsRef<Path>) {
    let link_path = link_path.as_ref();
    let metadata = std::fs::symlink_metadata(link_path)
        .unwrap_or_else(|_| panic!("symlink_metadata failed for {}", link_path.display()));
    assert!(
        metadata.file_type().is_file(),
        "expected file at {}, got {:?}",
        link_path.display(),
        metadata.file_type()
    );
    let expected_content =
        std::fs::read_to_string(expected_src).expect("failed to read expected src");
    let actual_content =
        std::fs::read_to_string(link_path).expect("failed to read copy destination");
    assert_eq!(
        actual_content,
        expected_content,
        "copy content mismatch at {}",
        link_path.display()
    );
}

/// 断言指定路径不存在。
pub fn assert_not_exists(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(
        !path.try_exists().unwrap_or(false),
        "expected {} to not exist",
        path.display()
    );
}

/// 断言指定路径存在（不关心类型）。
pub fn assert_exists(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(
        path.try_exists().unwrap_or(false),
        "expected {} to exist",
        path.display()
    );
}

/// 断言 track file 存在且包含指定数量的链接。
/// 返回 track file 路径。
pub fn assert_track_links(pack: &Path, expected_count: usize) -> PathBuf {
    use stow_cm::command::resolve_track_file;
    let track_path = resolve_track_file(pack).expect("resolve_track_file failed");
    assert!(
        track_path.try_exists().unwrap_or(false),
        "track file not found at {}",
        track_path.display()
    );
    let content = std::fs::read_to_string(&track_path).expect("failed to read track file");
    let track: stow_cm::track_file::Track =
        toml::from_str(&content).expect("failed to parse track file");
    assert_eq!(
        track.links.len(),
        expected_count,
        "expected {} links in track file, got {}",
        expected_count,
        track.links.len()
    );
    track_path
}

/// 在 TestEnv 的 config 目录下创建标准测试 pack，包含 stow-cm.toml + 两个文件。
///
/// 返回 `(pack_dir, target_dir)`。
pub fn setup_basic_pack(env: &TestEnv, pack_name: &str) -> (PathBuf, PathBuf) {
    let target_dir = env.config_path().join(pack_name);
    std::fs::create_dir_all(&target_dir).expect("failed to create target dir");

    let config = pack_config(&target_dir.to_string_lossy());
    let pack_dir = create_pack(env.config_path(), pack_name, &config);

    write_pack_file(&pack_dir, "file_a.txt", "content a\n");
    write_pack_file(&pack_dir, "sub/file_b.txt", "content b\n");

    (pack_dir, target_dir)
}
