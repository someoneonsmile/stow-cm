//! 集成测试：`status` 命令 — 检查已安装 pack 的状态一致性。
//!
//! 覆盖：OK、MISSING、DANGLING、OVERWRITTEN、DRIFT、--fix、空 paths（全部 pack）、--json。

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{TestEnv, create_pack, make_global_config, pack_config, write_pack_file};
use stow_cm::command;
use stow_cm::config::Config;

// ── 辅助函数 ──

/// 创建并安装一个测试 pack，返回 `(pack_dir, target_dir)`。
///
/// pack 源文件放在 `env.config_path()/.stow-src/<pack_name>/`，
/// target（symlink 目标）指向 `env.config_path()/<pack_name>/`。
fn setup_installed_pack(env: &TestEnv, pack_name: &str) -> (PathBuf, PathBuf) {
    let target_dir = env.config_path().join(pack_name);
    let pack_base = env.config_path().join(".stow-src");
    std::fs::create_dir_all(&pack_base).unwrap();

    let config_toml = pack_config(&target_dir.to_string_lossy());
    let pack_dir = create_pack(&pack_base, pack_name, &config_toml);

    write_pack_file(&pack_dir, "file_a.txt", "content a\n");
    write_pack_file(&pack_dir, "sub/file_b.txt", "content b\n");

    let global = make_global_config();
    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).unwrap());
    command::install(&config, &pack_dir, false).unwrap();

    (pack_dir, target_dir)
}

// ── 测试 ──

/// 已安装 pack 的状态检查：所有链接应为 OK。
#[test]
fn status_ok() {
    let env = TestEnv::new();
    let (pack_dir, _target_dir) = setup_installed_pack(&env, "test_pack");

    let global = make_global_config();
    // 状态检查不应出错（所有链接 OK）
    command::status(&global, vec![pack_dir.clone()], false, false).unwrap();
}

/// 手动删除 symlink 后状态检查：应报告 MISSING（函数本身不 panic）。
#[test]
fn status_missing() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 删除目标位置的 symlink
    let link_path = target_dir.join("file_a.txt");
    assert!(link_path.try_exists().unwrap());
    std::fs::remove_file(&link_path).unwrap();
    assert!(!link_path.try_exists().unwrap());

    let global = make_global_config();
    // 状态检查不应 panic，仅报告 MISSING
    command::status(&global, vec![pack_dir], false, false).unwrap();
}

/// 删除源文件后状态检查：应报告 DANGLING。
#[test]
fn status_dangling() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 验证 symlink 存在
    let link_path = target_dir.join("file_a.txt");
    common::assert_symlink(&link_path, pack_dir.join("file_a.txt"));

    // 删除 pack 中的源文件
    let src_path = pack_dir.join("file_a.txt");
    std::fs::remove_file(&src_path).unwrap();

    let global = make_global_config();
    // 状态检查不应 panic，仅报告 DANGLING
    command::status(&global, vec![pack_dir], false, false).unwrap();
}

/// 用普通文件替换 symlink 后状态检查：应报告 OVERWRITTEN。
#[test]
fn status_overwritten() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 删除 symlink，替换为普通文件
    let link_path = target_dir.join("file_a.txt");
    std::fs::remove_file(&link_path).unwrap();
    std::fs::write(&link_path, "replaced content\n").unwrap();
    assert!(link_path.is_file());

    let global = make_global_config();
    // 状态检查不应 panic，仅报告 OVERWRITTEN
    command::status(&global, vec![pack_dir], false, false).unwrap();
}

/// 修改 symlink 指向其他目标后状态检查：应报告 DRIFT。
#[test]
fn status_drift() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 修改 symlink 指向其他位置
    let link_path = target_dir.join("file_a.txt");
    std::fs::remove_file(&link_path).unwrap();
    let drift_target = env.config_path().join("drift_target.txt");
    std::fs::write(&drift_target, "drift content\n").unwrap();
    std::os::unix::fs::symlink(&drift_target, &link_path).unwrap();

    let global = make_global_config();
    // 状态检查不应 panic，仅报告 DRIFT
    command::status(&global, vec![pack_dir], false, false).unwrap();
}

/// `status --fix`：应自动修复 MISSING 链接。
#[test]
fn status_fix_recreates_missing_link() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 删除 symlink
    let link_path = target_dir.join("file_a.txt");
    std::fs::remove_file(&link_path).unwrap();
    assert!(!link_path.try_exists().unwrap());

    let global = make_global_config();
    // status --fix 应重新创建缺失的链接
    command::status(&global, vec![pack_dir.clone()], true, false).unwrap();

    // 验证缺失的链接已被修复
    common::assert_symlink(&link_path, pack_dir.join("file_a.txt"));
}

/// `status --fix` 不应修复 OVERWRITTEN（被普通文件覆盖）的链接。
#[test]
fn status_fix_does_not_fix_overwritten() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup_installed_pack(&env, "test_pack");

    // 删除 symlink，替换为普通文件
    let link_path = target_dir.join("file_a.txt");
    std::fs::remove_file(&link_path).unwrap();
    std::fs::write(&link_path, "replaced content\n").unwrap();
    assert!(link_path.is_file());

    let global = make_global_config();
    // status --fix 不应覆盖已有的普通文件
    command::status(&global, vec![pack_dir], true, false).unwrap();

    // 验证普通文件仍然存在（未被修复为 symlink）
    assert!(link_path.is_file());
    let content = std::fs::read_to_string(&link_path).unwrap();
    assert_eq!(content, "replaced content\n");
}

/// `status` 不传 paths：检查所有已安装 pack。
#[test]
fn status_all_packs() {
    let env = TestEnv::new();
    setup_installed_pack(&env, "pack_a");
    setup_installed_pack(&env, "pack_b");

    let global = make_global_config();
    // 空 paths 扫描所有 pack，不应出错
    command::status(&global, vec![], false, false).unwrap();
}

/// `status --json`：输出 JSON 格式，不应出错。
#[test]
fn status_json() {
    let env = TestEnv::new();
    let (pack_dir, _target_dir) = setup_installed_pack(&env, "test_pack");

    let global = make_global_config();
    // JSON 输出模式不应出错
    command::status(&global, vec![pack_dir], false, true).unwrap();
}
