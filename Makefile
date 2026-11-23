.PHONY: frontend build release test run clean

frontend:
	cd frontend && bun install && bun run build

build: frontend
	cargo build

release: frontend
	cargo build --release
	@echo "Single binary ready at: ./target/release/bunamateur"

test:
	cargo test

run: build
	./target/debug/bunamateur

clean:
	cargo clean
	rm -rf frontend/dist
