PREFIX ?= /usr/local
BINDIR ?= $(PREFIX)/bin
CARGO ?= cargo

.PHONY: build test install uninstall clean

build:
	$(CARGO) build --release

test:
	$(CARGO) test

install: build
	install -d "$(BINDIR)"
	install -m 755 target/release/tson "$(BINDIR)/tson"

uninstall:
	rm -f "$(BINDIR)/tson"

clean:
	$(CARGO) clean
