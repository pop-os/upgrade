export prefix ?= /usr
sysconfdir ?= /usr/share
bindir = $(prefix)/bin
includedir = $(prefix)/include
libdir = $(prefix)/lib

PACKAGE=pop_upgrade_gtk
LIB=lib$(PACKAGE).so
BIN=pop-upgrade
ID=com.system76.PopUpgrade

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

VENDOR ?= 0
ifeq ($(VENDOR),1)
endif

BINARY=target/$(TARGET)/$(BIN)
LIBRARY=target/$(TARGET)/$(LIB)
PKGCONFIG = target/$(PACKAGE).pc
HEADER = gtk/ffi/$(PACKAGE).h
NOTIFY = pop-upgrade-notify
NOTIFY_APPID = $(ID).Notify
STARTUP_DESKTOP = $(NOTIFY_APPID).desktop

.PHONY: all clean distclean install uninstall update vendor

all: $(BINARY) $(LIBRARY) $(PKGCONFIG)

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
ifeq ($(VENDOR),1)
	rm -rf vendor; tar pxf vendor.tar
endif

install:
	install -Dm0755 "$(BINARY)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -Dm0755 "data/$(BIN).sh" "$(DESTDIR)$(libdir)/$(BIN)/upgrade.sh"
	install -Dm0644 "$(HEADER)" "$(DESTDIR)$(includedir)/$(PACKAGE).h"
	install -Dm0644 "$(LIBRARY)" "$(DESTDIR)$(libdir)/$(LIB)"
	install -Dm0644 "$(PKGCONFIG)" "$(DESTDIR)$(libdir)/pkgconfig/$(PACKAGE).pc"
	install -Dm0644 "data/$(BIN)-init.service" "$(DESTDIR)$(libdir)/systemd/system/$(BIN)-init.service"
	install -Dm0644 "data/$(BIN).conf" "$(DESTDIR)$(sysconfdir)/dbus-1/system.d/$(BIN).conf"
	install -Dm0644 "data/$(BIN).service" "$(DESTDIR)$(libdir)/systemd/system/$(BIN).service"
	install -Dm0644 "data/dbus-$(BIN).service" "$(DESTDIR)$(sysconfdir)/dbus-1/system-services/$(ID).service"
	install -Dm0644 "data/$(NOTIFY).service" "$(DESTDIR)$(libdir)/systemd/user/$(NOTIFY).service"
	install -Dm0644 "data/$(NOTIFY).timer" "$(DESTDIR)$(libdir)/systemd/user/$(NOTIFY).timer"
	install -Dm0644 "data/$(STARTUP_DESKTOP)" "$(DESTDIR)/etc/xdg/autostart/$(STARTUP_DESKTOP)"

$(BINARY): extract-vendor
	cargo build $(ARGS) -p pop-upgrade

$(LIBRARY): extract-vendor
	cargo build $(ARGS) -p pop-upgrade-gtk-ffi

$(PKGCONFIG):
	echo "libdir=$(libdir)" > "$@.partial"
	echo "includedir=$(includedir)" >> "$@.partial"
	cat "$(PKGCONFIG).stub" >> "$@.partial"
	mv "$@.partial" "$@"
