# Makefile for stow-cm

BINARY   := stow-cm
CARGO    := cargo
CROSS    := cross
PREFIX   ?= /usr
BINDIR   := $(PREFIX)/bin
DATADIR  := $(PREFIX)/share
MANDIR   := $(DATADIR)/man
SHELL_HELP_DIR := shell_help
RELEASE  := target/release

VERSION ?= $(shell grep '^version' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
DATE   ?= $(shell date +%Y%m%d)

.PHONY: all build build-cross check test install uninstall clean fmt fmt-check lint ci run dev help \
        aur-srcinfo aur-push aur-release \
        aur-nightly-srcinfo aur-nightly-push aur-nightly-release

all: build

# --- Build ---

build: build-completions

build-cross:
	$(CROSS) build --release --target x86_64-unknown-linux-musl

check:
	$(CARGO) check

test:
	$(CARGO) test

# --- Install ---

install:
	@test -f $(RELEASE)/$(BINARY) || { echo "Run 'make' first."; exit 1; }
	@test -d $(SHELL_HELP_DIR) || { echo "Run 'make' first."; exit 1; }
	install -Dm755 $(RELEASE)/$(BINARY) $(DESTDIR)$(BINDIR)/$(BINARY)
	install -Dm644 $(SHELL_HELP_DIR)/complete/$(BINARY).bash $(DESTDIR)$(DATADIR)/bash-completion/completions/$(BINARY)
	install -Dm644 $(SHELL_HELP_DIR)/complete/_$(BINARY) $(DESTDIR)$(DATADIR)/zsh/site-functions/_$(BINARY)
	install -Dm644 $(SHELL_HELP_DIR)/complete/$(BINARY).fish $(DESTDIR)$(DATADIR)/fish/vendor_completions.d/$(BINARY).fish
	install -Dm644 $(SHELL_HELP_DIR)/man/$(BINARY).1 $(DESTDIR)$(MANDIR)/man1/$(BINARY).1

# --- Uninstall ---

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(BINARY)
	rm -f $(DESTDIR)$(DATADIR)/bash-completion/completions/$(BINARY)
	rm -f $(DESTDIR)$(DATADIR)/zsh/site-functions/_$(BINARY)
	rm -f $(DESTDIR)$(DATADIR)/fish/vendor_completions.d/$(BINARY).fish
	rm -f $(DESTDIR)$(MANDIR)/man1/$(BINARY).1

# --- Helpers ---

build-completions:
	SHELL_HELP_DIR=$(SHELL_HELP_DIR) $(CARGO) build --release

# --- Maintenance ---

clean:
	$(CARGO) clean
	rm -rf $(SHELL_HELP_DIR)

fmt:
	$(CARGO) fmt --all

fmt-check:
	$(CARGO) fmt --all -- --check

lint:
	$(CARGO) clippy --all-features

ci: check test fmt-check lint

# --- Dev ---

run:
	$(CARGO) run --

dev:
	RUST_LOG=debug $(CARGO) run --

# --- Help ---

help:
	@echo "Usage:"
	@echo "  make                  Build release binary"
	@echo "  sudo make install     Install to system directory"
	@echo "  make uninstall        Remove installed files"
	@echo "  make clean            Remove build artifacts"
	@echo "  make fmt              Format code"
	@echo "  make lint             Run clippy"
	@echo "  make ci               Run all CI checks"
	@echo ""
	@echo "Variables:"
	@echo "  PREFIX    Install prefix (default: /usr)"
	@echo "  DESTDIR   Staging directory (for packaging)"
	@echo ""
	@echo "Examples:"
	@echo "  make && sudo make install              Build and install to /usr"
	@echo "  make && sudo make install PREFIX=/usr/local  Install to /usr/local"
	@echo "  make install PREFIX=~/.local           Install to user directory"
	@echo "  make install DESTDIR=/tmp/pkg          Stage install"
	@echo ""
	@echo "AUR targets:"
	@echo "  make aur-srcinfo             Regenerate aur/.SRCINFO"
	@echo "  make aur-push                Commit and push aur/ to AUR"
	@echo "  make aur-release             Update version + push to AUR (VERSION=...)"
	@echo "  make aur-nightly-srcinfo     Regenerate aur-nightly/.SRCINFO"
	@echo "  make aur-nightly-push        Commit and push aur-nightly/ to AUR"
	@echo "  make aur-nightly-release     Update date + push to nightly AUR (DATE=...)"
	@echo ""
	@echo "AUR variables:"
	@echo "  VERSION   Version for aur-release    (default: from Cargo.toml)"
	@echo "  DATE      Date for aur-nightly-release (default: today)"

# --- AUR: Stable ---

# 重新生成 aur/.SRCINFO（手动修改 PKGBUILD 后执行）
aur-srcinfo:
	cd aur && makepkg --printsrcinfo > .SRCINFO

# 提交 aur/ 变更并推送到 AUR
aur-push:
	git add aur/
	git commit -m "chore: update AUR package" || true
	git subtree push --prefix=aur aur master

# 发布新版本到 AUR（用法: make aur-release VERSION=0.18.0）
aur-release:
	sed -i 's/^pkgver=.*/pkgver=$(VERSION)/' aur/PKGBUILD
	sed -i 's/^pkgrel=.*/pkgrel=1/' aur/PKGBUILD
	cd aur && makepkg --printsrcinfo > .SRCINFO
	git add aur/
	git commit -m "chore: update AUR to $(VERSION)" || true
	git subtree push --prefix=aur aur master

# --- AUR: Nightly ---

# 重新生成 aur-nightly/.SRCINFO
aur-nightly-srcinfo:
	cd aur-nightly && makepkg --printsrcinfo > .SRCINFO

# 提交 aur-nightly/ 变更并推送到 Nightly AUR
aur-nightly-push:
	git add aur-nightly/
	git commit -m "chore: update nightly AUR package" || true
	git subtree push --prefix=aur-nightly aur-nightly master

# 发布 Nightly AUR（用法: make aur-nightly-release DATE=20260703）
aur-nightly-release:
	sed -i 's/^pkgver=.*/pkgver=$(DATE)/' aur-nightly/PKGBUILD
	sed -i 's/^pkgrel=.*/pkgrel=1/' aur-nightly/PKGBUILD
	cd aur-nightly && makepkg --printsrcinfo > .SRCINFO
	git add aur-nightly/
	git commit -m "chore: update nightly AUR to $(DATE)" || true
	git subtree push --prefix=aur-nightly aur-nightly master
