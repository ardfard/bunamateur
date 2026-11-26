IMAGE ?= bunamateur:latest
AUDIO_ARGS ?=
KACHE := $(shell command -v kache 2>/dev/null)

ifneq ($(KACHE),)
export RUSTC_WRAPPER ?= kache
endif

.PHONY: frontend build release test test-frontend clippy check run docker audio-dry-run cache-stats cache-doctor clean

frontend:
	cd frontend && bun install && bun run build

build: frontend
	cargo build

release: frontend
	cargo build --release
	@echo "Single binary ready at: ./target/release/bunamateur"

test:
	cargo test

test-frontend:
	cd frontend && bun test

clippy:
	cargo clippy --all-targets -- -D warnings

check: test-frontend frontend clippy test

run: build
	./target/debug/bunamateur

docker:
	docker build -t $(IMAGE) .

audio-dry-run:
	bun run scripts/generate_audio.ts --dry-run $(AUDIO_ARGS)

cache-stats:
	@if [ -n "$(KACHE)" ]; then kache stats; else echo "kache not installed (install via: cargo install kache --locked)"; fi

cache-doctor:
	@if [ -n "$(KACHE)" ]; then kache doctor; else echo "kache not installed (install via: cargo install kache --locked)"; fi

clean:
	cargo clean
	rm -rf frontend/dist
