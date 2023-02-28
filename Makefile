.PHONY: build check clean clippy compile-cairo compile-starknet coverage deps deps-macos remove-venv test


OS := $(shell uname)
ifeq ($(OS), Darwin)
	CFLAGS  += -I/opt/homebrew/opt/gmp/include
	LDFLAGS += -L/opt/homebrew/opt/gmp/lib
endif


CAIRO_SOURCES=$(wildcard cairo_programs/*.cairo)
CAIRO_TARGETS=$(patsubst %.cairo,%.json,$(CAIRO_SOURCES))

STARKNET_SOURCES=$(wildcard tests/*.cairo)
STARKNET_TARGETS=$(patsubst %.cairo,%.json,$(STARKNET_SOURCES))


#
# VENV rules.
#

deps-venv:
	pip install \
		fastecdsa \
		cairo-lang==0.10.3

compile-cairo: $(CAIRO_TARGETS)
compile-starknet: $(STARKNET_TARGETS)
	starknet-compile \
		--account_contract \
		starknet_programs/account_without_validation.cairo \
		--output starknet_programs/account_without_validation.json

	starknet-compile \
		starknet_programs/test_contract.cairo \
		--output starknet_programs/test_contract.json
	cd starknet_programs/ && starknet-compile \
		blockifier/ERC20_without_some_syscalls/ERC20/ERC20.cairo \
		--output erc20_contract_without_some_syscalls.json


cairo_programs/%.json: cairo_programs/%.cairo
	cairo-compile $< --output $@ || rm $@

tests/%.json: tests/%.cairo
	starknet-compile --debug_info_with_source $< --output $@ || rm $@


#
# Normal rules.
#

build:
	cargo build --release

check:
	cargo check

deps:
	cargo install cargo-tarpaulin --version 0.23.1
	python3 -m venv starknet-venv
	. starknet-venv/bin/activate && $(MAKE) deps-venv


clean:
	rm cairo_programs/*json
	-rm -rf starknet-venv/
	-rm -f cairo_programs/*.json
	-rm -f starknet_programs/*.json
	-rm -f tests/*.json

clippy:
	cargo clippy --all-targets -- -D warnings

test:
	. starknet-venv/bin/activate && $(MAKE) compile-cairo compile-starknet
	cargo test

coverage:
	. starknet-venv/bin/activate && $(MAKE) compile-cairo compile-starknet
	cargo tarpaulin
	-rm -f default.profraw
