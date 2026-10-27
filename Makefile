BINARY       := ibis
PREFIX       ?= /usr/local
BINDIR       ?= $(PREFIX)/bin
DATADIR      ?= $(PREFIX)/share
APPDIR       ?= $(DATADIR)/applications
ICONDIR      ?= $(DATADIR)/icons/hicolor/256x256/apps
CARGO        ?= cargo
CARGO_FLAGS  ?=
ARGS         ?=
PROFILE      ?= debug
REPO_ROOT    := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
DESKTOP_FILE := $(REPO_ROOT)packaging/ibis.desktop
ICON_FILE    := $(REPO_ROOT)ui/assets/appicon.png

# Cargo calls its normal debug profile "dev"; its output is target/debug.
ifeq ($(PROFILE),debug)
  PROFILE_FLAGS :=
else ifeq ($(PROFILE),release)
  PROFILE_FLAGS := --release
else
  $(error PROFILE must be debug or release)
endif

.PHONY: dev build release run install uninstall clean check-deps check fmt

dev: run

build:
	cd "$(REPO_ROOT)" && $(CARGO) build --locked $(PROFILE_FLAGS) $(CARGO_FLAGS)

release:
	$(MAKE) build PROFILE=release

run: build
	cd "$(REPO_ROOT)" && ./target/$(PROFILE)/$(BINARY) $(ARGS)

check-deps:
	bash "$(REPO_ROOT)scripts/check-linux-deps.sh" --build

install: build
	install -Dm755 "$(REPO_ROOT)target/$(PROFILE)/$(BINARY)" "$(DESTDIR)$(BINDIR)/$(BINARY)"
	install -Dm644 "$(DESKTOP_FILE)" "$(DESTDIR)$(APPDIR)/ibis.desktop"
	install -Dm644 "$(ICON_FILE)" "$(DESTDIR)$(ICONDIR)/$(BINARY).png"
	if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$(APPDIR)"; fi
	if [ -z "$(DESTDIR)" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t "$(DATADIR)/icons/hicolor"; fi

uninstall:
	rm -f "$(DESTDIR)$(BINDIR)/$(BINARY)"
	rm -f "$(DESTDIR)$(APPDIR)/ibis.desktop"
	rm -f "$(DESTDIR)$(ICONDIR)/$(BINARY).png"
	if [ -z "$(DESTDIR)" ] && command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database "$(APPDIR)"; fi
	if [ -z "$(DESTDIR)" ] && command -v gtk-update-icon-cache >/dev/null 2>&1; then gtk-update-icon-cache -q -t "$(DATADIR)/icons/hicolor"; fi

clean:
	cd "$(REPO_ROOT)" && $(CARGO) clean

check:
	cd "$(REPO_ROOT)" && $(CARGO) check --locked $(CARGO_FLAGS)

fmt:
	cd "$(REPO_ROOT)" && $(CARGO) fmt --all
