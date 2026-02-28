prefix ?= /usr
exec_prefix = $(prefix)
bindir = $(exec_prefix)/bin
libdir = $(exec_prefix)/lib
includedir = $(prefix)/include
datadir = $(prefix)/share

SRC = Cargo.toml Cargo.lock Makefile $(shell find src -type f -wholename '*src/*.rs')

.PHONY: all clean distclean install uninstall update gui install-gui

BIN=powercurve
GUI_BIN=powercurve-gui
ID=com.vintagetechie.PowerCurve
GUI_ID=com.vintagetechie.PowerCurveGui

DEBUG ?= 0
ifeq ($(DEBUG),0)
	ARGS += "--release"
	TARGET = release
endif

VENDOR ?= 0
ifeq ($(VENDOR),1)
	ARGS += "--frozen"
endif

all: target/release/$(BIN)

clean:
	cargo clean

distclean:
	rm -rf .cargo vendor vendor.tar.xz

install: all
	install -D -m 0644 "data/$(ID).conf" "$(DESTDIR)$(datadir)/dbus-1/system.d/$(ID).conf"
	install -D -m 0644 "data/$(ID).service" "$(DESTDIR)$(libdir)/systemd/system/$(ID).service"
	install -D -m 0644 "data/$(ID).xml" "$(DESTDIR)$(datadir)/dbus-1/interfaces/$(ID).xml"
	install -D -m 0644 "data/powercurve-monitor.service" "$(DESTDIR)$(libdir)/systemd/user/powercurve-monitor.service"
	install -D -m 0755 "target/release/$(BIN)" "$(DESTDIR)$(bindir)/$(BIN)"
	install -D -m 0644 "man/$(BIN).1" "$(DESTDIR)$(datadir)/man/man1/$(BIN).1"
	install -d "$(DESTDIR)$(datadir)/doc/$(BIN)/examples"
	install -m 0644 examples/fan-*.toml "$(DESTDIR)$(datadir)/doc/$(BIN)/examples/"

uninstall:
	rm -f "$(DESTDIR)$(bindir)/$(BIN)"
	rm -f "$(DESTDIR)$(datadir)/dbus-1/interfaces/$(ID).xml"
	rm -f "$(DESTDIR)$(datadir)/dbus-1/system.d/$(ID).conf"
	rm -f "$(DESTDIR)$(libdir)/systemd/system/$(ID).service"
	rm -f "$(DESTDIR)$(libdir)/systemd/user/powercurve-monitor.service"
	rm -f "$(DESTDIR)$(datadir)/man/man1/$(BIN).1"
	rm -rf "$(DESTDIR)$(datadir)/doc/$(BIN)"

update:
	cargo update

vendor:
	mkdir -p .cargo
	cargo vendor | head -n -1 > .cargo/config
	echo 'directory = "vendor"' >> .cargo/config
	tar pcfJ vendor.tar.xz vendor
	rm -rf vendor

gui: target/release/$(GUI_BIN)

install-gui: gui
	install -D -m 0755 "target/release/$(GUI_BIN)" "$(DESTDIR)$(bindir)/$(GUI_BIN)"
	install -D -m 0644 "gui/resources/$(GUI_ID).desktop" "$(DESTDIR)$(datadir)/applications/$(GUI_ID).desktop"
	install -D -m 0644 "gui/resources/$(GUI_ID).metainfo.xml" "$(DESTDIR)$(datadir)/appdata/$(GUI_ID).metainfo.xml"
	install -D -m 0644 "gui/resources/icons/hicolor/scalable/apps/icon.svg" "$(DESTDIR)$(datadir)/icons/hicolor/scalable/apps/$(GUI_ID).svg"

target/release/$(BIN): $(SRC)
ifeq ($(VENDOR),1)
	tar pxf vendor.tar.xz
endif
	cargo build $(ARGS)

target/release/$(GUI_BIN): $(SRC) gui/Cargo.toml $(shell find gui/src -type f -name '*.rs')
ifeq ($(VENDOR),1)
	tar pxf vendor.tar.xz
endif
	cargo build -p $(GUI_BIN) $(ARGS)
