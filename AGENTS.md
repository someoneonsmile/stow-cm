# AGENTS.md - stow-cm

## Project Overview

A Rust-based config manager (GNU Stow-like) that creates symlinks from a source pack directory to target locations. Supports encryption, custom init/clear scripts, and multiple install modes.

## Essential Commands

### Build & Development

```bash
# Check
cargo check

# Build (debug)
cargo build

# Build release (optimized, uses cross for musl targets)
cargo build --release

# Run with logging
RUST_LOG=debug cargo run -- install ./mypack
```

### Code Quality (CI-required)

```bash
# Format (enforced in CI)
cargo fmt --all -- --check

# Clippy (strict: pedantic + many denies)
cargo clippy --all-features

# Tests (minimal test coverage currently)
cargo test
```

**CI order**: `cargo check` → `cargo test` → `cargo fmt --check` → `cargo clippy`

## Architecture

### Source Layout

```
src/
├── main.rs        # Entry point, tokio async runtime
├── cli.rs         # Clap CLI definitions (used by build.rs)
├── config.rs      # Config parsing, merging, defaults
├── constants.rs   # Paths, env vars, defaults
├── command/       # Core commands: install, remove, reload, clean, encrypt, decrypt
├── executor.rs    # Async execution orchestration
├── symlink.rs     # Symlink operations (symlink vs copy modes)
├── track_file.rs  # State tracking in $XDG_STATE_HOME
├── merge.rs       # Config merge strategies
├── merge_tree.rs  # Directory tree merging
├── crypto.rs      # Encryption (ChaCha20-Poly1305, AES-GCM)
├── base64.rs      # Base64 utilities
├── util.rs        # Path/canonicalization helpers
├── error.rs       # anyhow Result type
├── custom_type.rs # Type wrappers
└── dev.rs         # Dev utilities
```

### Key Design Patterns

1. **Async throughout**: Uses `tokio` for async file operations and subprocess execution
2. **Config merging**: Layered config system (global → XDG → pack → defaults) using custom `Merge` trait
3. **Track file**: State stored in `$XDG_STATE_HOME/stow-cm/{PACK_ID}/track.toml`
4. **Symlink modes**: `Symlink` (default) or `Copy` mode per pack
5. **Script execution**: Supports Bin, Shell, ShellStr, Make, Python, Lua init/clear scripts

## Critical Constraints

### Clippy (Strict)

- `pedantic` level with `priority = -1` (deny all)
- Explicitly denies: `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented`, `indexing_slicing`
- **Never use**: `.unwrap()`, `.expect()`, `panic!`, `todo!`, `unimplemented!`, `panic!`
- Tests exempt via `clippy.toml`: `allow-unwrap-in-tests = true`

### Rust Version

- Edition: `2024`
- Toolchain: `stable` (see `rust-toolchain` file)

### Formatting

- 4-space indent for `.rs` files (2-space for others, per `.editorconfig`)
- Unix newlines
- `group_imports = "StdExternalCrate"` enforced

## Build System

### Build Script (`build.rs`)

Generates shell completions and man pages at build time:
- Shell completions: `SHELL_HELP_DIR` or `OUT_DIR/complete/`
- Man page: `SHELL_HELP_DIR/man/` or `OUT_DIR/man/`

Uses `Cross.toml` to pass `SHELL_HELP_DIR` through cross-compilation.

### Release Profile

- `opt-level = "s"` (size optimized)
- `lto = true`, `panic = "abort"`, `strip = true`

## Testing

**Status**: Minimal test coverage currently (only 4 test modules found).

Tests exist in:
- `config.rs`: Config merging
- `merge_tree.rs`: Tree merging
- `base64.rs`: Base64 round-trip
- `crypto.rs`: Crypto operations

Run single test:
```bash
cargo test config_merge
```

## Common Pitfalls

1. **Config file location**: Pack config must be at `{pack_dir}/stow-cm.toml`, NOT in subdirectories
2. **UNSET_VALUE**: Use `"!"` in config to explicitly unset a value to None
3. **Env var expansion**: Uses `shellexpand` with default syntax `${VAR:-default}`
4. **Pack ID**: Hash of pack path, used for state tracking
5. **Cross-compilation**: Release builds use `cross` for musl targets

## Dependencies to Know

- `tokio` - Async runtime (full features)
- `clap` - CLI with derive macros
- `serde` + `toml` - Config serialization
- `merge` - Custom fork for config merging
- `ring` - Cryptographic primitives
- `walkdir` - Directory traversal
- `anyhow` - Error handling

## CI Workflows

- `ci.yml`: Main CI (check, test, fmt, clippy) on PR/push to main
- `release.yml`: Multi-platform release builds (linux-musl, macos) on tag push
- `nightly.yml`: Nightly builds
