prefix ?= /usr
sysconfdir ?= /etc
exec_prefix = $(prefix)
bindir = $(exec_prefix)/bin
libdir = $(exec_prefix)/lib
includedir = $(prefix)/include
datarootdir = $(prefix)/share
datadir = $(datarootdir)

BIN=pop-upgrade
TARGET = debug

TESTING ?= 0
ifeq ($(TESTING),1)
	ARGS += --features testing
endif

DEBUG ?= 0
ifeq ($(DEBUG),0)
	ARGS += "--release"
	TARGET = release
endif

VENDORED ?= 0
ifeq ($(VENDORED),1)
	ARGS += "--frozen"
endif

.PHONY: all clean distclean install uninstall update

all: target/$(TARGET)/$(BIN)

clean:
	cargo clean

distclean:
	rm -rf .cargo vendor vendor.tar.xz

vendor:
	mkdir -p .cargo
	cargo vendor | head -n -1 > .cargo/config
	echo 'directory = "vendor"' >> .cargo/config
	tar pcfJ vendor.tar.xz vendor
	rm -rf vendor

install: all
	install -Dm04755 "target/$(TARGET)/$(BIN)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -Dm04755 "data/$(BIN).sh" "$(DESTDIR)$(libdir)/$(BIN)/upgrade.sh"
	install -Dm0644 "data/$(BIN).service" "$(DESTDIR)/lib/systemd/system/$(BIN).service"
	install -Dm0644 "data/$(BIN)-init.service" "$(DESTDIR)/lib/systemd/system/$(BIN)-init.service"
	install -Dm0644 "data/$(BIN).conf" "$(DESTDIR)$(sysconfdir)/dbus-1/system.d/$(BIN).conf"

target/$(TARGET)/$(BIN): Makefile Cargo.lock Cargo.toml src/* src/*/*
ifeq ($(VENDORED),1)
	ls
	tar pxf vendor.tar.xz
endif
	cargo build $(ARGS)
