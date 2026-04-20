# AGENTS.md - stow-cm

A gnu-stow-like config manager written in Rust. Single binary crate.

## Build & Test Commands

```bash
make              # Build release binary + shell completions/man pages
make test         # cargo test
make ci           # check + test + fmt-check + clippy (full CI pipeline)
make lint         # cargo clippy --all-features
make fmt          # cargo fmt --all
make run          # cargo run --
make dev          # RUST_LOG=debug cargo run --   # with debug logging
make clean        # cargo clean + rm shell_help/
```

**Important**: `make build-completions` (called by `make`) runs `cargo build --release` with `SHELL_HELP_DIR=shell_help` to generate shell completions and man pages into `shell_help/`. This directory is gitignored.

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
- **Command dispatch**: `src/command.rs` - maps CLI commands to implementation
- **Execution**: `src/executor.rs` - parallel execution of pack operations
- **Key modules**: `config`, `crypto`, `merge_tree`, `symlink`, `track_file`, `util`

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
