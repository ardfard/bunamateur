IMAGE ?= bunamateur:latest
AUDIO_ARGS ?=

.PHONY: frontend build release test test-frontend clippy check run docker audio-dry-run clean

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

clean:
	cargo clean
	rm -rf frontend/dist
