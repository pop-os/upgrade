prefix ?= /usr
sysconfdir ?= /etc
bindir = $(prefix)/bin
includedir = $(prefix)/include
libdir = $(prefix)/lib

SRC = Makefile Cargo.lock Cargo.toml $(shell find src -type f -wholename '*src/*.rs')
LIB_SRC = $(SRC) gtk/Cargo.toml gtk/ffi/Cargo.toml $(shell find gtk -type f -wholename '*src/*.rs')

PACKAGE=pop_upgrade_gtk
LIB=lib$(PACKAGE).so
BIN=pop-upgrade

TESTING ?= 0
ifeq ($(TESTING),1)
	ARGS += --features testing
endif

DEBUG ?= 0
TARGET = debug
ifeq ($(DEBUG),0)
	ARGS += "--release"
	TARGET = release
endif

VENDORED ?= 0
ifeq ($(VENDORED),1)
	ARGS += "--frozen"
endif

BINARY=target/$(TARGET)/$(BIN)
LIBRARY=target/$(TARGET)/$(LIB)
PKGCONFIG = target/$(PACKAGE).pc
HEADER = gtk/ffi/$(PACKAGE).h
NOTIFY = pop-upgrade-notify
NOTIFY_APPID = com.system76.PopUpgrade.Notify
STARTUP_DESKTOP = $(NOTIFY_APPID).desktop

.PHONY: all clean distclean install uninstall update

all: $(BINARY) $(LIBRARY) $(PKGCONFIG) target/$(NOTIFY).service target/$(STARTUP_DESKTOP)

clean:
	cargo clean

distclean:
	rm -rf .cargo vendor vendor.tar target

vendor:
	mkdir -p .cargo
	cargo vendor | head -n -1 > .cargo/config
	echo 'directory = "vendor"' >> .cargo/config
	tar pcf vendor.tar vendor
	rm -rf vendor

extract-vendor:
ifeq ($(VENDORED),1)
	rm -rf vendor; tar pxf vendor.tar
endif

install:
	install -Dm04755 "$(BINARY)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -Dm04755 "data/$(BIN).sh" "$(DESTDIR)$(libdir)/$(BIN)/upgrade.sh"
	install -Dm0644 "data/$(BIN).service" "$(DESTDIR)$(libdir)/systemd/system/$(BIN).service"
	install -Dm0644 "data/$(BIN)-init.service" "$(DESTDIR)$(libdir)/systemd/system/$(BIN)-init.service"
	install -Dm0644 "data/$(BIN).conf" "$(DESTDIR)$(sysconfdir)/dbus-1/system.d/$(BIN).conf"
	install -Dm0644 "$(LIBRARY)" "$(DESTDIR)$(libdir)/$(LIB)"
	install -Dm0644 "$(PKGCONFIG)" "$(DESTDIR)$(libdir)/pkgconfig/$(PACKAGE).pc"
	install -Dm0644 "$(HEADER)" "$(DESTDIR)$(includedir)/$(PACKAGE).h"
	install -Dm0644 "target/$(NOTIFY).service" "$(DESTDIR)$(libdir)/systemd/user/$(NOTIFY).service"
	install -Dm0644 "target/$(NOTIFY).timer" "$(DESTDIR)$(libdir)/systemd/user/$(NOTIFY).timer"
	install -Dm0644 "target/$(STARTUP_DESKTOP)" "$(DESTDIR)/etc/xdg/autostart/$(STARTUP_DESKTOP)"

$(BINARY): $(SRC) extract-vendor
	cargo build $(ARGS)

$(LIBRARY): $(LIB_SRC) extract-vendor
	cargo build $(ARGS) -p pop-upgrade-gtk-ffi

target/$(NOTIFY).service: Makefile tools/src/notify.rs extract-vendor
	env prefix=$(prefix) cargo run -p tools --bin notify-gen $(ARGS)

notify-desktop target/$(STARTUP_DESKTOP): Makefile tools/src/desktop_entry.rs extract-vendor
	cargo run -p tools --bin desktop-entry $(ARGS) -- \
		--appid $(NOTIFY_APPID) \
		--name "Pop!_OS Release Check" \
		--icon distributor-logo-upgrade-symbolic \
		--comment "Check for a new OS release, and display notification if found" \
		--categories System \
		--binary pop-upgrade \
		--args "release check" \
		--prefix $(prefix)

$(PKGCONFIG):
	echo "libdir=$(libdir)" > "$@.partial"
	echo "includedir=$(includedir)" >> "$@.partial"
	cat "$(PKGCONFIG).stub" >> "$@.partial"
	mv "$@.partial" "$@"
