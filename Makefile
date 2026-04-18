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

.PHONY: all build build-cross check test install uninstall clean fmt fmt-check lint ci run dev help

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
