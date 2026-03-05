PREFIX ?= /usr
BINDIR = $(PREFIX)/bin
DATADIR = $(PREFIX)/share
APPLICATIONSDIR = $(DATADIR)/applications
ICONSDIR = $(DATADIR)/icons/hicolor/scalable/apps
METAINFODIR = $(DATADIR)/metainfo

APP_ID = dev.oblivius.spacecal-for-monado
BIN_NAME = spacecal-for-monado

.PHONY: all build install install-user uninstall uninstall-user clean

all: build

build:
	cargo build --release --locked

install: build
	install -Dm755 target/release/$(BIN_NAME) $(DESTDIR)$(BINDIR)/$(BIN_NAME)
	install -Dm644 data/$(APP_ID).desktop $(DESTDIR)$(APPLICATIONSDIR)/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).svg $(DESTDIR)$(ICONSDIR)/$(APP_ID).svg
	install -Dm644 data/$(APP_ID).metainfo.xml $(DESTDIR)$(METAINFODIR)/$(APP_ID).metainfo.xml

install-user: build
	install -Dm755 target/release/$(BIN_NAME) $(HOME)/.local/bin/$(BIN_NAME)
	install -Dm644 data/$(APP_ID).desktop $(HOME)/.local/share/applications/$(APP_ID).desktop
	install -Dm644 data/$(APP_ID).svg $(HOME)/.local/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	install -Dm644 data/$(APP_ID).metainfo.xml $(HOME)/.local/share/metainfo/$(APP_ID).metainfo.xml

uninstall:
	rm -f $(DESTDIR)$(BINDIR)/$(BIN_NAME)
	rm -f $(DESTDIR)$(APPLICATIONSDIR)/$(APP_ID).desktop
	rm -f $(DESTDIR)$(ICONSDIR)/$(APP_ID).svg
	rm -f $(DESTDIR)$(METAINFODIR)/$(APP_ID).metainfo.xml

uninstall-user:
	rm -f $(HOME)/.local/bin/$(BIN_NAME)
	rm -f $(HOME)/.local/share/applications/$(APP_ID).desktop
	rm -f $(HOME)/.local/share/icons/hicolor/scalable/apps/$(APP_ID).svg
	rm -f $(HOME)/.local/share/metainfo/$(APP_ID).metainfo.xml

clean:
	cargo clean
