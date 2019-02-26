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

DEBUG ?= 0
ifeq ($(DEBUG),0)
	ARGS = "--release"
	TARGET = release
endif

VENDORED ?= 0
ifneq ($(VENDORED),0)
	ARGS += "--frozen"
endif

.PHONY: all clean distclean install uninstall update

all: target/$(TARGET)/$(BIN)

clean:
	cargo clean

install: all
	install -Dm04755 "target/$(TARGET)/$(BIN)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -Dm04755 "data/$(BIN).sh" "$(DESTDIR)$(libdir)/$(BIN)/upgrade.sh"
	install -Dm0644 "data/$(SERVICE)" "$(DESTDIR)/lib/systemd/system/$(BIN).service"
	install -Dm0644 "data/$(SERVICE)" "$(DESTDIR)/lib/systemd/system/$(BIN)-init.service"
	install -Dm0644 "data/$(BIN).conf" "$(DESTDIR)$(sysconfdir)/dbus-1/system.d/$(BIN).conf"

target/$(TARGET)/$(BIN): Cargo.lock Cargo.toml src/* src/*/*
	cargo build $(ARGS)
