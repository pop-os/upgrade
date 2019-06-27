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

.PHONY: all clean distclean install uninstall update

all: $(BINARY) $(LIBRARY) $(PKGCONFIG)

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

install:
	install -Dm04755 "$(BINARY)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -Dm04755 "data/$(BIN).sh" "$(DESTDIR)$(libdir)/$(BIN)/upgrade.sh"
	install -Dm0644 "data/$(BIN).service" "$(DESTDIR)/lib/systemd/system/$(BIN).service"
	install -Dm0644 "data/$(BIN)-init.service" "$(DESTDIR)/lib/systemd/system/$(BIN)-init.service"
	install -Dm0644 "data/$(BIN).conf" "$(DESTDIR)$(sysconfdir)/dbus-1/system.d/$(BIN).conf"
	install -Dm0644 "$(LIBRARY)" "$(DESTDIR)$(libdir)/$(LIB)"
	install -Dm0644 "$(PKGCONFIG)" "$(DESTDIR)$(libdir)/pkgconfig/$(PACKAGE).pc"
	install -Dm0644 "$(HEADER)" "$(DESTDIR)$(includedir)/$(PACKAGE).h"

$(BINARY): $(SRC)
	cargo build $(ARGS)

$(LIBRARY): $(LIB_SRC)
	cargo build $(ARGS) -p pop-upgrade-gtk-ffi

$(PKGCONFIG):
	echo "libdir=$(libdir)" > "$@.partial"
	echo "includedir=$(includedir)" >> "$@.partial"
	cat "$(PKGCONFIG).stub" >> "$@.partial"
	mv "$@.partial" "$@"
