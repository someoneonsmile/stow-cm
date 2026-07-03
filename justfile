# Justfile for stow-cm
#
# Usage:
#   just                  Build release binary
#   sudo just install     Install to system directory
#   just uninstall        Remove installed files
#   just clean            Remove build artifacts
#   just fmt              Format code
#   just lint             Run clippy
#   just ci               Run all CI checks
#
# Variables:
#   PREFIX    Install prefix (default: /usr)
#   DESTDIR   Staging directory (for packaging)
#
# Examples:
#   just && sudo just install               Build and install to /usr
#   just && sudo just install PREFIX=/usr/local  Install to /usr/local
#   just install PREFIX=~/.local            Install to user directory
#   just install DESTDIR=/tmp/pkg           Stage install
#
# AUR targets:
#   just aur-srcinfo             Regenerate aur/.SRCINFO
#   just aur-push                Commit and push aur/ to AUR
#   just aur-release             Update version + push to AUR (VERSION=...)
#   just aur-nightly-srcinfo     Regenerate aur-nightly/.SRCINFO
#   just aur-nightly-push        Commit and push aur-nightly/ to AUR
#   just aur-nightly-release     Update date + push to nightly AUR
#
# AUR variables:
#   VERSION   Version for aur-release    (default: from Cargo.toml)

# ---- Variables ----

PREFIX := "/usr"
BINDIR := PREFIX / "bin"
DATADIR := PREFIX / "share"
MANDIR := DATADIR / "man"
SHELL_HELP_DIR := "shell_help"
RELEASE := "target" / "release"
BINARY := "stow-cm"

VERSION := `grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/'`
DESTDIR := ""

# ---- Default ----

default:
    @just --list

# ---- Build ----

build:
    SHELL_HELP_DIR={{ SHELL_HELP_DIR }} cargo build --release

check:
    cargo check

test:
    cargo test

# ---- Install ----

install: build
    #!/usr/bin/env bash
    set -euo pipefail
    install -Dm755 {{ RELEASE }}/{{ BINARY }} {{ DESTDIR }}{{ BINDIR }}/{{ BINARY }}
    install -Dm644 {{ SHELL_HELP_DIR }}/complete/{{ BINARY }}.bash {{ DESTDIR }}{{ DATADIR }}/bash-completion/completions/{{ BINARY }}
    install -Dm644 {{ SHELL_HELP_DIR }}/complete/_{{ BINARY }} {{ DESTDIR }}{{ DATADIR }}/zsh/site-functions/_{{ BINARY }}
    install -Dm644 {{ SHELL_HELP_DIR }}/complete/{{ BINARY }}.fish {{ DESTDIR }}{{ DATADIR }}/fish/vendor_completions.d/{{ BINARY }}.fish
    install -Dm644 {{ SHELL_HELP_DIR }}/man/{{ BINARY }}.1 {{ DESTDIR }}{{ MANDIR }}/man1/{{ BINARY }}.1

# ---- Uninstall ----

uninstall:
    rm -f {{ DESTDIR }}{{ BINDIR }}/{{ BINARY }}
    rm -f {{ DESTDIR }}{{ DATADIR }}/bash-completion/completions/{{ BINARY }}
    rm -f {{ DESTDIR }}{{ DATADIR }}/zsh/site-functions/_{{ BINARY }}
    rm -f {{ DESTDIR }}{{ DATADIR }}/fish/vendor_completions.d/{{ BINARY }}.fish
    rm -f {{ DESTDIR }}{{ MANDIR }}/man1/{{ BINARY }}.1

# ---- Maintenance ----

clean:
    cargo clean
    rm -rf {{ SHELL_HELP_DIR }}

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

lint:
    cargo clippy --all-features

ci: check test fmt-check lint

# ---- Dev ----

run:
    cargo run --

dev:
    RUST_LOG=debug cargo run --

# ---- AUR: Stable ----

# Regenerate aur/.SRCINFO (run after manually editing PKGBUILD)
aur-srcinfo:
    cd aur && makepkg --printsrcinfo > .SRCINFO

# Commit aur/ changes and push to AUR
aur-push:
    git add aur/
    git commit -m "chore: update AUR package" || true
    git subtree push --prefix=aur aur master

# Publish new version to AUR (usage: just aur-release VERSION=0.18.0)
aur-release:
    sed -i 's/^pkgver=.*/pkgver={{ VERSION }}/' aur/PKGBUILD
    sed -i 's/^pkgrel=.*/pkgrel=1/' aur/PKGBUILD
    cd aur && makepkg --printsrcinfo > .SRCINFO
    git add aur/
    git commit -m "chore: update AUR to {{ VERSION }}" || true
    git subtree push --prefix=aur aur master

# ---- AUR: Nightly ----

# Regenerate aur-nightly/.SRCINFO
aur-nightly-srcinfo:
    cd aur-nightly && makepkg --printsrcinfo > .SRCINFO

# Commit aur-nightly/ changes and push to Nightly AUR
aur-nightly-push:
    git add aur-nightly/
    git commit -m "chore: update nightly AUR package" || true
    git subtree push --prefix=aur-nightly aur-nightly master

# Publish Nightly AUR
aur-nightly-release:
    #!/usr/bin/env bash
    set -euo pipefail
    DATE=$(date +%Y%m%d)
    sed -i "s/^pkgver=.*/pkgver=${DATE}/" aur-nightly/PKGBUILD
    sed -i 's/^pkgrel=.*/pkgrel=1/' aur-nightly/PKGBUILD
    cd aur-nightly && makepkg --printsrcinfo > .SRCINFO
    git add aur-nightly/
    git commit -m "chore: update nightly AUR to ${DATE}" || true
    git subtree push --prefix=aur-nightly aur-nightly master
