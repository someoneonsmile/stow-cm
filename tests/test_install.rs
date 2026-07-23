#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use stow_cm::command::install;

// ── 基础安装 ──

#[test]
fn test_install_basic() {
    let env = common::TestEnv::new();
    let pack_name = "basicpack";
    let target_dir = env.config_path().join("basic_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    let config_toml = common::pack_config(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "readme.txt", "hello\n");
    common::write_pack_file(&pack_dir, "LICENSE", "MIT\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, false).expect("install should succeed");

    // 验证两个 symlink
    common::assert_symlink(target_dir.join("readme.txt"), pack_dir.join("readme.txt"));
    common::assert_symlink(target_dir.join("LICENSE"), pack_dir.join("LICENSE"));
    common::assert_track_links(&pack_dir, 2);
}

// ── Dry Run ──

#[test]
fn test_install_dry_run() {
    let env = common::TestEnv::new();
    let pack_name = "drypack";
    let target_dir = env.config_path().join("dry_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    let config_toml = common::pack_config(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "notes.txt", "dry run content\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, true).expect("dry run install should succeed");

    // 目标目录中不应创建任何 symlink
    common::assert_not_exists(target_dir.join("notes.txt"));
    // track file 也不应创建
    let track_file =
        stow_cm::command::resolve_track_file(&pack_dir).expect("track path");
    common::assert_not_exists(&track_file);
}

// ── 冲突检测 ──

#[test]
fn test_install_conflict() {
    let env = common::TestEnv::new();
    let pack_name = "conflictpack";
    let target_dir = env.config_path().join("conflict_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    // 预先在目标目录中创建一个文件（与 pack 中的文件同名）
    common::write_pack_file(&target_dir, "notes.txt", "pre-existing content\n");

    let config_toml = common::pack_config(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "notes.txt", "pack content\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    let result = install(&config, &pack_dir, false);
    assert!(result.is_err(), "install should fail on conflict");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("check conflict"),
        "error should mention conflict: {err_msg}"
    );
}

// ── Override 覆盖 ──

#[test]
fn test_install_override() {
    let env = common::TestEnv::new();
    let pack_name = "overridepack";
    let target_dir = env.config_path().join("override_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    // 预先创建文件（将被覆盖）
    common::write_pack_file(&target_dir, "notes.txt", "pre-existing\n");

    let config_toml =
        common::pack_config_with_override(&target_dir.to_string_lossy(), &[r".*\.txt"]);
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "notes.txt", "pack content\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, false).expect("override install should succeed");

    // 验证 symlink 已覆盖原有文件
    common::assert_symlink(target_dir.join("notes.txt"), pack_dir.join("notes.txt"));
    common::assert_track_links(&pack_dir, 1);
}

// ── Ignore 忽略 ──

#[test]
fn test_install_ignore() {
    let env = common::TestEnv::new();
    let pack_name = "ignorepack";
    let target_dir = env.config_path().join("ignore_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    // 忽略 .md 文件
    let config_toml = common::pack_config_with_ignore(&target_dir.to_string_lossy(), &[r".*\.md"]);
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "readme.md", "# Readme\n");
    common::write_pack_file(&pack_dir, "config.lua", "return {}\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, false).expect("ignore install should succeed");

    // 被忽略的文件不应链接
    common::assert_not_exists(target_dir.join("readme.md"));
    // 未被忽略的文件应正常链接
    common::assert_symlink(target_dir.join("config.lua"), pack_dir.join("config.lua"));
    common::assert_track_links(&pack_dir, 1);
}

// ── Fold 目录折叠 ──

#[test]
fn test_install_fold() {
    let env = common::TestEnv::new();
    let pack_name = "foldpack";
    let target_dir = env.config_path().join("fold_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    let config_toml = common::pack_config(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "docs/guide.txt", "guide content\n");
    common::write_pack_file(&pack_dir, "config.lua", "return {}\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, false).expect("fold install should succeed");

    // fold 开启：`docs/` 整个目录折叠为单个 symlink
    common::assert_symlink(target_dir.join("docs"), pack_dir.join("docs"));
    common::assert_symlink(target_dir.join("config.lua"), pack_dir.join("config.lua"));
    common::assert_track_links(&pack_dir, 2);
}

// ── Copy 模式 ──

#[test]
fn test_install_copy_mode() {
    let env = common::TestEnv::new();
    let pack_name = "copypack";
    let target_dir = env.config_path().join("copy_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    let config_toml = common::pack_config_copy(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "notes.txt", "hello copy mode\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    install(&config, &pack_dir, false).expect("copy mode install should succeed");

    // copy 模式：目标应为普通文件（非 symlink），内容与源文件一致
    common::assert_copy(target_dir.join("notes.txt"), pack_dir.join("notes.txt"));
    common::assert_track_links(&pack_dir, 1);
}

// ── 幂等性：重复安装失败 ──

#[test]
fn test_install_idempotency() {
    let env = common::TestEnv::new();
    let pack_name = "idempack";
    let target_dir = env.config_path().join("idem_target");

    std::fs::create_dir_all(&target_dir).expect("create target dir");

    let config_toml = common::pack_config(&target_dir.to_string_lossy());
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "notes.txt", "idempotent\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    // 第一次安装成功
    install(&config, &pack_dir, false).expect("first install should succeed");
    common::assert_symlink(target_dir.join("notes.txt"), pack_dir.join("notes.txt"));

    // 第二次安装应失败（track file 已存在）
    let result = install(&config, &pack_dir, false);
    assert!(result.is_err(), "second install should fail");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("has been install"),
        "error should mention already installed: {err_msg}"
    );
}

// ── Target=None（跳过链接） ──

#[test]
fn test_install_target_none() {
    let env = common::TestEnv::new();
    let pack_name = "none_target_pack";

    // 使用 target='!' — 经过 finalize 后 target 变为 None
    let config_toml = common::pack_config("!");
    let pack_dir = common::create_pack(env.state_path(), pack_name, &config_toml);
    common::write_pack_file(&pack_dir, "unused.txt", "should not be linked\n");

    let global = common::make_global_config();
    let config =
        stow_cm::config::Config::for_pack(&pack_dir, &global, None, false).expect("config");
    let config = Arc::new(config);

    // 安装应成功，但不创建任何链接
    install(&config, &pack_dir, false).expect("target=None install should succeed");

    // track file 不应存在
    let track_file =
        stow_cm::command::resolve_track_file(&pack_dir).expect("track path");
    common::assert_not_exists(&track_file);
}
