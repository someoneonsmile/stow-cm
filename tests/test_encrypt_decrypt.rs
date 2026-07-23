//! 加密/解密集成测试。
//!
//! 覆盖：
//! - encrypt → decrypt 往返验证
//! - 空 pack 加密
//! - 二进制文件跳过
//! - ignore 模式跳过
//! - 加密 pack 安装完整生命周期

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use stow_cm::command::{decrypt, encrypt, install, remove};
use stow_cm::config::Config;

// ── 辅助函数 ──

/// 为加密 pack 构建 `Arc<Config>`。
fn for_crypto_pack(pack_dir: &std::path::Path, global: &Config) -> Arc<Config> {
    Arc::new(Config::for_pack(pack_dir, global, None, false).expect("for_pack"))
}

// ─────────────────────────────────────────────
// 测试 1: encrypt → decrypt 往返
// ─────────────────────────────────────────────

/// 对含有 `&{...}` 标记的文件先 encrypt 再 decrypt，验证内容完全恢复。
#[test]
fn encrypt_decrypt_roundtrip() {
    let env = common::TestEnv::new();
    let (key_path, _key) = common::create_test_key(&env);

    let target_dir = env.config_path().join("roundtrip");
    std::fs::create_dir_all(&target_dir).unwrap();

    let config_toml =
        common::pack_config_encrypted(&target_dir.to_string_lossy(), &key_path.to_string_lossy());
    let pack_dir = common::create_pack(env.config_path(), "roundtrip", &config_toml);

    let original = "public &{secret data} public\n";
    common::write_pack_file(&pack_dir, "secret.txt", original);

    let global = common::make_global_config();
    let config = for_crypto_pack(&pack_dir, &global);

    // encrypt
    encrypt(&config, &pack_dir).expect("encrypt should succeed");

    let encrypted_content =
        std::fs::read_to_string(pack_dir.join("secret.txt")).expect("read encrypted file");
    assert_ne!(
        encrypted_content, original,
        "encrypt should change file content"
    );
    assert!(
        encrypted_content.contains("&{"),
        "encrypted content should retain left boundary"
    );
    assert!(
        encrypted_content.contains('}'),
        "encrypted content should retain right boundary"
    );

    // decrypt
    decrypt(&config, &pack_dir).expect("decrypt should succeed");

    let decrypted_content =
        std::fs::read_to_string(pack_dir.join("secret.txt")).expect("read decrypted file");
    assert_eq!(
        decrypted_content, original,
        "decrypt should restore original content exactly"
    );
}

// ─────────────────────────────────────────────
// 测试 2: 空 pack 加密
// ─────────────────────────────────────────────

/// 对没有任何文件的 pack 执行 encrypt，不应报错。
#[test]
fn encrypt_empty_pack() {
    let env = common::TestEnv::new();
    let (key_path, _key) = common::create_test_key(&env);

    let target_dir = env.config_path().join("empty");
    std::fs::create_dir_all(&target_dir).unwrap();

    let config_toml =
        common::pack_config_encrypted(&target_dir.to_string_lossy(), &key_path.to_string_lossy());
    let pack_dir = common::create_pack(env.config_path(), "empty", &config_toml);

    let global = common::make_global_config();
    let config = for_crypto_pack(&pack_dir, &global);

    encrypt(&config, &pack_dir).expect("encrypt on empty pack should succeed");
}

// ─────────────────────────────────────────────
// 测试 3: 二进制文件跳过
// ─────────────────────────────────────────────

/// encrypt 应跳过非 UTF-8 二进制文件，不报错、不修改。
#[test]
fn encrypt_skips_binary() {
    let env = common::TestEnv::new();
    let (key_path, _key) = common::create_test_key(&env);

    let target_dir = env.config_path().join("binary");
    std::fs::create_dir_all(&target_dir).unwrap();

    let config_toml =
        common::pack_config_encrypted(&target_dir.to_string_lossy(), &key_path.to_string_lossy());
    let pack_dir = common::create_pack(env.config_path(), "binary", &config_toml);

    // 写入非 UTF-8 字节
    let binary_content: &[u8] = &[0xFF, 0xFE, 0x00, 0x01, 0x02, 0x03];
    common::write_pack_file(&pack_dir, "data.bin", binary_content);

    let global = common::make_global_config();
    let config = for_crypto_pack(&pack_dir, &global);

    encrypt(&config, &pack_dir).expect("encrypt with binary file should succeed");

    let content = std::fs::read(pack_dir.join("data.bin")).expect("read binary file");
    assert_eq!(
        content, binary_content,
        "binary file should not be modified by encrypt"
    );
}

// ─────────────────────────────────────────────
// 测试 4: ignore 模式跳过
// ─────────────────────────────────────────────

/// 匹配 ignore 正则的文件应被 encrypt 跳过。
#[test]
fn encrypt_skips_ignored() {
    let env = common::TestEnv::new();
    let (key_path, _key) = common::create_test_key(&env);

    let target_dir = env.config_path().join("ignored");
    std::fs::create_dir_all(&target_dir).unwrap();

    // 在加密配置基础上叠加 ignore 规则
    let config_toml = format!(
        r#"target = '{}'
mode = 'symlink'
ignore = ['secret\.ignore']

[encrypted]
enable = true
key_path = '{}'
left_boundary = '&{{'
right_boundary = '}}'
encrypted_alg = 'ChaCha20-Poly1305'
"#,
        target_dir.to_string_lossy(),
        key_path.to_string_lossy()
    );
    let pack_dir = common::create_pack(env.config_path(), "ignored", &config_toml);

    // 应被 ignore 跳过的文件
    common::write_pack_file(&pack_dir, "secret.ignore", "&{should not change}");

    // 应被 encrypt 处理的文件
    common::write_pack_file(&pack_dir, "secret.txt", "&{should change}");

    let global = common::make_global_config();
    let config = for_crypto_pack(&pack_dir, &global);

    encrypt(&config, &pack_dir).expect("encrypt should succeed");

    // 被 ignore 的文件内容不变
    let ignored = std::fs::read_to_string(pack_dir.join("secret.ignore")).expect("read ignored");
    assert_eq!(
        ignored, "&{should not change}",
        "ignored file should not be encrypted"
    );

    // 未被 ignore 的文件内容被加密
    let encrypted = std::fs::read_to_string(pack_dir.join("secret.txt")).expect("read encrypted");
    assert_ne!(
        encrypted, "&{should change}",
        "non-ignored file should be encrypted"
    );
    assert!(
        encrypted.contains("&{"),
        "encrypted content should still have boundaries"
    );
}

// ─────────────────────────────────────────────
// 测试 5: 加密 pack 安装完整生命周期
// ─────────────────────────────────────────────

/// 加密 pack 的完整生命周期：encrypt → install → 验证解密的符号链接 → remove → 验证清理。
#[test]
fn encrypted_install_full_lifecycle() {
    let env = common::TestEnv::new();
    let (key_path, _key) = common::create_test_key(&env);

    let target_dir = env.config_path().join("crypto-lifecycle");
    std::fs::create_dir_all(&target_dir).unwrap();

    // pack 放在 state 下，与 target 分离
    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).unwrap();
    let pack_dir = pack_base.join("crypto-lifecycle");
    std::fs::create_dir_all(&pack_dir).unwrap();

    let config_toml =
        common::pack_config_encrypted(&target_dir.to_string_lossy(), &key_path.to_string_lossy());
    std::fs::write(pack_dir.join("stow-cm.toml"), config_toml).expect("write config");

    let original = "public &{my secret} public\n";
    common::write_pack_file(&pack_dir, "secret.txt", original);

    let pack_dir = std::fs::canonicalize(&pack_dir).expect("canonicalize");

    let global = common::make_global_config();
    let config = for_crypto_pack(&pack_dir, &global);

    encrypt(&config, &pack_dir).expect("encrypt should succeed");
    let encrypted =
        std::fs::read_to_string(pack_dir.join("secret.txt")).expect("read encrypted pack file");
    assert_ne!(encrypted, original, "encrypt should modify file content");

    install(&config, &pack_dir, false).expect("install encrypted pack should succeed");

    let link_path = target_dir.join("secret.txt");
    assert!(link_path.exists(), "symlink should exist in target");

    let symlink_target = std::fs::read_link(&link_path).expect("read symlink target");
    assert!(
        !symlink_target.starts_with(&pack_dir),
        "symlink should point to decrypted path, not pack dir\n  link: {}\n  pack: {}",
        symlink_target.display(),
        pack_dir.display()
    );

    let decrypted_content =
        std::fs::read_to_string(&link_path).expect("read decrypted target file");
    assert_eq!(
        decrypted_content, "public my secret public\n",
        "installed file should have decrypted content without boundaries"
    );

    common::assert_track_links(&pack_dir, "crypto-lifecycle", 1);

    remove(&config, &pack_dir, false).expect("remove encrypted pack should succeed");
    common::assert_not_exists(&link_path);
}
