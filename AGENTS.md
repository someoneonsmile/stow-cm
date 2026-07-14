# AGENTS.md - stow-cm

A gnu-stow-like config manager written in Rust. Single binary crate.

## Build & Test Commands

```bash
just              # Build release binary + shell completions/man pages
just test         # cargo test
just ci           # check + test + fmt-check + clippy (full CI pipeline)
just lint         # cargo clippy --all-features
just fmt          # cargo fmt --all
just run          # cargo run --
just dev          # RUST_LOG=debug cargo run --   # with debug logging
just clean        # cargo clean + rm shell_help/
```

**Important**: `just build` runs `cargo build --release` with `SHELL_HELP_DIR=shell_help` to generate shell completions and man pages into `shell_help/`. This directory is gitignored.

## Rust Toolchain

- **Edition**: 2024 (very recent; some Rust features may be unstable)
- **Channel**: stable
- **Cross-compilation**: Uses `cross` for musl target (`x86_64-unknown-linux-musl`)

## Clippy Strictness

`Cargo.toml` enables `pedantic` lint with many `deny` rules including:
- `unwrap_used = "deny"`
- `expect_used = "deny"`
- `todo = "deny"`
- `panic = "deny"`

Exceptions in `clippy.toml`: `allow-unwrap-in-tests` and `allow-expect-in-tests` are enabled.

## Architecture

- **Entry point**: `src/main.rs` - async main using tokio
- **CLI**: `src/cli.rs` - clap derive, commands: `install`, `remove`, `reload`, `clean`, `encrypt`, `decrypt`
- **Command dispatch**: `src/command/` - maps CLI commands to implementation
  - `mod.rs` — 模块入口，re-export 所有公开命令 + 公共 helper（`pack_envs`, `resolve_track_file`, `reload`）
  - `install.rs` — `install()` + `install_link()`
  - `remove.rs` — `remove()` + `remove_link()`
  - `clean.rs` — `clean()` + `clean_link()`
  - `crypto.rs` — `encrypt()` + `decrypt()` + `crypto_process()`
  - 新增命令（`status`, `adopt`, `list`, `init`, `doctor`, `export` 等）按此模式各放独立文件，在 `mod.rs` 中声明 `mod xyz;` 并 `pub use`
- **Execution**: `src/executor.rs` - parallel execution of pack operations
- **Key modules**: `config`, `crypto`, `merge_tree`, `symlink`, `track_file`, `util`
- **功能规划**: `.omo/stow-cm-feature-roadmap.md`

## Config File Locations

- **Global config**: `$XDG_CONFIG_HOME/stow-cm/config.toml`
- **Pack config**: `{pack_dir}/stow-cm.toml` (NOT `pack_dir/pack_sub_path/stow-cm.toml`)

## CI/Release

- **CI** (push to main, PRs): `cargo check` → `cargo test` → `cargo fmt --all -- --check` → `cargo clippy --all-features`
- **Release**: Tag `*.*.*` triggers `release.yml` - builds linux-musl + macOS binaries via `cross`
- **Nightly**: `workflow_dispatch` on `nightly.yml` - bumps version with date+commit and creates prerelease

## Build-Time Code Generation

`build.rs` generates shell completions (bash, zsh, fish) and man pages via clap_complete and clap_mangen. The `SHELL_HELP_DIR` env var controls output directory. This runs automatically during `cargo build`.

## Musl Allocator

For `target_env = "musl"` builds, `main.rs` configures `mimalloc` as the global allocator due to performance concerns with musl's default allocator.
