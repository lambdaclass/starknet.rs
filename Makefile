.PHONY: build check deps deps-macos clean compile_starknet clippy remove-venv venv-test test clean coverage
	
build: 
	cargo build --release

check: 
	cargo check 

deps:
	cargo install cargo-tarpaulin --version 0.23.1 && \
	python3 -m venv starknet-in-rs-venv
	. starknet-in-rs-venv/bin/activate && \
	pip install cairo_lang==0.10.1 && \
	deactivate

deps-macos:
	cargo install cargo-tarpaulin --version 0.23.1 && \
	python3 -m venv starknet-in-rs-venv
	. starknet-in-rs-venv/bin/activate && \
	CFLAGS=-I/opt/homebrew/opt/gmp/include LDFLAGS=-L/opt/homebrew/opt/gmp/lib pip install fastecdsa cairo_lang==0.10.1 && \
	deactivate

clean: 
	rm cairo_syscalls/*json
	rm cairo_programs/*json

compile_starknet:
	cairo-compile cairo_programs/syscalls.cairo --output cairo_programs/syscalls.json && \
	cairo-compile cairo_programs/contracts.cairo > cairo_programs/contracts.json && \
	starknet-compile starknet_programs/fibonacci.cairo > starknet_programs/fibonacci.json 

clippy:
	cargo clippy --all-targets -- -D warnings

remove-venv:
	rm -rf starknet-in-rs-venv

venv-test:
	. starknet-in-rs-venv/bin/activate && \
	cairo-compile cairo_syscalls/syscalls.cairo --output cairo_syscalls/syscalls.json && \
	starknet-compile starknet_programs/fibonacci.cairo > starknet_programs/fibonacci.json && \
	starknet-compile starknet_programs/contracts.cairo > starknet_programs/contracts.json && \
	cargo test

test:
	cargo test

coverage:
	cargo tarpaulin
	rm -f default.profraw
