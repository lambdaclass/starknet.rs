.PHONY: usage build check clean clippy compile-cairo compile-starknet \
		 compile-cairo-2-casm compile-cairo-2-sierra coverage deps test heaptrack check-python-version

export PATH:=$(shell pyenv root)/shims:$(PATH)
export PYENV_VERSION=3.9

OS := $(shell uname)
ifeq ($(OS), Darwin)
	export CFLAGS  += -I/opt/homebrew/opt/gmp/include
	export LDFLAGS += -L/opt/homebrew/opt/gmp/lib
endif

CAIRO_SOURCES=$(wildcard cairo_programs/*.cairo)
CAIRO_TARGETS=$(patsubst %.cairo,%.json,$(CAIRO_SOURCES))
CAIRO_ABI_TARGETS=$(patsubst %.cairo,%_abi.json,$(CAIRO_SOURCES))

STARKNET_SOURCES=$(wildcard starknet_programs/*.cairo)
STARKNET_TARGETS=$(patsubst %.cairo,%.json,$(STARKNET_SOURCES))
STARKNET_ABI_TARGETS=$(patsubst %.cairo,%_abi.json,$(STARKNET_SOURCES))

BUILTIN_SOURCES=$(wildcard starknet_programs/*.cairo)
BUILTIN_TARGETS=$(patsubst %.cairo,%.json,$(BUILTIN_SOURCES))

STARKNET_COMPILE_CAIRO_2:=cairo2/bin/starknet-compile
STARKNET_SIERRA_COMPILE_CAIRO_2:=cairo2/bin/starknet-sierra-compile

usage:
	@echo 'Usage:'
	@echo '    build:           Builds the Rust code'
	@echo '    check:           Runs cargo check'
	@echo '    deps:            Installs dependencies'
	@echo '    deps-macos:      Installs depedencies for MacOS'
	@echo '    clean:           Cleans all build artifacts'
	@echo '    clippy:          Runs clippy'
	@echo '    test:            Runs all tests'
	@echo '    test-cairo-2:    Runs the Cairo 2 tests'
	@echo '    test-doctests:   Runs the doctests'
	@echo '    coverage:        Runs everything necessary to generate the coverage report'
	@echo '    coverage-report: Just generates the coverage report'
	@echo '    heaptrack:       Runs the heaptrack script'
	@echo '    flamegraph:      Runs cargo flamegraph'
	@echo '    benchmark:       Runs the benchmarks scripts'

#
# VENV rules.
#

deps-venv:
	pip install -r requirements.txt

compile-cairo: $(CAIRO_TARGETS) $(CAIRO_ABI_TARGETS)
compile-starknet: $(STARKNET_TARGETS) $(STARKNET_ABI_TARGETS)

cairo_programs/%.json cairo_programs/%_abi.json: cairo_programs/%.cairo
	. starknet-venv/bin/activate && cd cairo_programs/ && cairo-compile $(shell grep "^// @compile-flags += .*$$" $< | cut -c 22-) ../$< --output ../$@ || rm ../$@

starknet_programs/%.json starknet_programs/%_abi.json: starknet_programs/%.cairo
	. starknet-venv/bin/activate && \
	cd starknet_programs/ && \
	starknet-compile $(shell grep "^// @compile-flags += .*$$" $< | cut -c 22-) \
	../$< \
	--no_debug_info \
	--output ./$*.json \
	--abi ./$*_abi.json \
	|| rm ./$*.json ./$*_abi.json
# Compiles .cairo files into .json files. if the command fails, then it removes all of the .json files

# ======================
# Test Cairo 2 Contracts
# ======================

CAIRO_2_CONTRACTS_TEST_DIR=starknet_programs/cairo2
CAIRO_2_CONTRACTS_TEST_CAIRO_FILES:=$(wildcard $(CAIRO_2_CONTRACTS_TEST_DIR)/*.cairo)
CAIRO_2_COMPILED_SIERRA_CONTRACTS:=$(patsubst $(CAIRO_2_CONTRACTS_TEST_DIR)/%.cairo, $(CAIRO_2_CONTRACTS_TEST_DIR)/%.sierra, $(CAIRO_2_CONTRACTS_TEST_CAIRO_FILES))
CAIRO_2_COMPILED_CASM_CONTRACTS:= $(patsubst $(CAIRO_2_CONTRACTS_TEST_DIR)/%.sierra, $(CAIRO_2_CONTRACTS_TEST_DIR)/%.casm, $(CAIRO_2_COMPILED_SIERRA_CONTRACTS))

$(CAIRO_2_CONTRACTS_TEST_DIR)/%.sierra: $(CAIRO_2_CONTRACTS_TEST_DIR)/%.cairo
	$(STARKNET_COMPILE_CAIRO_2) --single-file $< $@ --replace-ids

$(CAIRO_2_CONTRACTS_TEST_DIR)/%.casm: $(CAIRO_2_CONTRACTS_TEST_DIR)/%.sierra
	$(STARKNET_SIERRA_COMPILE_CAIRO_2) --add-pythonic-hints $< $@

compile-cairo-2-sierra: $(CAIRO_2_COMPILED_SIERRA_CONTRACTS)
compile-cairo-2-casm: $(CAIRO_2_COMPILED_CASM_CONTRACTS)

CAIRO_2_VERSION=2.6.3

cairo-repo-2-dir = cairo2
cairo-repo-2-dir-macos = cairo2-macos

build-cairo-2-compiler-macos: | $(cairo-repo-2-dir-macos)

$(cairo-repo-2-dir-macos): cairo-${CAIRO_2_VERSION}-macos.tar
	$(MAKE) decompress-cairo SOURCE=$< TARGET=cairo2/

build-cairo-2-compiler: | $(cairo-repo-2-dir)

$(cairo-repo-2-dir): cairo-${CAIRO_2_VERSION}.tar
	$(MAKE) decompress-cairo SOURCE=$< TARGET=cairo2/

decompress-cairo:
	rm -rf $(TARGET) \
	&& tar -xzvf $(SOURCE) \
	&& mv cairo/ $(TARGET)

cairo-%-macos.tar:
	curl -L -o "$@" "https://github.com/starkware-libs/cairo/releases/download/v$*/release-aarch64-apple-darwin.tar"

cairo-%.tar:
	curl -L -o "$@" "https://github.com/starkware-libs/cairo/releases/download/v$*/release-x86_64-unknown-linux-musl.tar.gz"

# =============================
# Test Cairo Kakarot Contracts
# =============================

KAKAROT_VERSION=v0.1.8
KAKAROT_DIR=starknet_programs/kakarot

KAKAROT_FILES:=$(wildcard $(KAKAROT_DIR)/*.json)
KAKAROT_COMPILED_SIERRA_CONTRACTS:=$(patsubst $(KAKAROT_DIR)/%.contract_class.json, $(KAKAROT_DIR)/%.sierra, $(KAKAROT_FILES))
KAKAROT_COMPILED_CASM_CONTRACTS:=$(patsubst $(KAKAROT_DIR)/%.compiled_contract_class.json, $(KAKAROT_DIR)/%.casm, $(KAKAROT_FILES))

build-kakarot: | $(KAKAROT_DIR)

compile-kakarot-sierra: $(KAKAROT_COMPILED_SIERRA_CONTRACTS)
compile-kakarot-casm: $(KAKAROT_COMPILED_CASM_CONTRACTS)

$(KAKAROT_DIR):
	rm -fr $(KAKAROT_DIR) \
	&& mkdir -p $(KAKAROT_DIR) \
	&& curl -L -o $(KAKAROT_DIR)/artifacts.zip "https://github.com/kkrt-labs/kakarot-ssj/releases/download/$(KAKAROT_VERSION)/dev-artifacts.zip" \
	&& $(MAKE) decompress-kakarot

decompress-kakarot:
	unzip $(KAKAROT_DIR)/artifacts.zip -d $(KAKAROT_DIR) \
	&& rm $(KAKAROT_DIR)/artifacts.zip \
	&& rm $(KAKAROT_DIR)/*.sierra.json \
	&& rm $(KAKAROT_DIR)/*.starknet_artifacts.json

$(KAKAROT_DIR)/%.sierra: $(KAKAROT_DIR)/%.contract_class.json
	mv $< $@

$(KAKAROT_DIR)/%.casm: $(KAKAROT_DIR)/%.compiled_contract_class.json
	mv $< $@

# =================
# Normal rules.
# =================

build: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo build --release --workspace

check: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo check --workspace --all-targets

deps: check-python-version build-cairo-2-compiler
	cargo install flamegraph --version 0.6.5 --locked
	cargo install cargo-llvm-cov --version 0.6.10 --locked
	-pyenv && pyenv install -s pypy3.9-7.3.9
	-pyenv && pyenv install -s 3.9.15
	python3.9 -m venv starknet-venv
	. starknet-venv/bin/activate && $(MAKE) deps-venv
	cargo install cargo-nextest --version 0.9.72 --locked

deps-macos: check-python-version build-cairo-2-compiler-macos
	cargo install flamegraph --version 0.6.5 --locked
	cargo install cargo-llvm-cov --version 0.6.10 --locked
	-pyenv install -s pypy3.9-7.3.9
	-pyenv install -s 3.9.15
	python3.9 -m venv starknet-venv
	. starknet-venv/bin/activate && $(MAKE) deps-venv
	cargo install cargo-nextest --locked

clean:
	-rm -rf starknet-venv/
	-rm -f cairo_programs/*.json
	-rm -f starknet_programs/*.json
	-rm -f starknet_programs/cairo2/*.casm
	-rm -f starknet_programs/cairo2/*.sierra
	-rm -f tests/*.json
	-rm -rf cairo2/
	-rm -rf cairo-*.tar

clippy: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo clippy --workspace --all-targets --all-features -- -D warnings

test: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo nextest run --workspace --all-targets --features=metrics,cairo-native

test-cairo-native: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo nextest run --workspace --test tests --features=cairo-native integration_tests::cairo_native

deps-kakarot: build-kakarot

test-kakarot: compile-kakarot-sierra compile-kakarot-casm
	cargo test --test tests --features=cairo-native test_kakarot_contract

test-doctests:
	cargo test --workspace --doc

coverage: compile-cairo compile-starknet compile-cairo-2-casm
	$(MAKE) coverage-report

coverage-report: compile-cairo compile-starknet compile-cairo-2-casm compile-cairo-2-sierra
	cargo llvm-cov nextest --lcov --ignore-filename-regex 'main.rs' --output-path lcov.info --release

heaptrack:
	./scripts/heaptrack.sh

flamegraph: compile-cairo compile-starknet
	CARGO_PROFILE_RELEASE_DEBUG=true cargo flamegraph --root --bench internals

benchmark: compile-cairo compile-starknet
	cargo build --release --all-targets
	./scripts/bench-invoke.sh
	./scripts/bench-deploy-invoke.sh
	./scripts/bench-fibonacci.sh
	./scripts/bench-deploy.sh
