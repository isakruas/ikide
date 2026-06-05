.PHONY: all build run clean

all: build

# Compiles the IDE using Docker (same way the main project does)
build:
	@echo "Building ikide (via Docker)..."
	docker run --rm -v "$$(pwd):/workspace" -w /workspace rust:latest cargo build --release
	docker run --rm -v "$$(pwd):/workspace" -w /workspace rust:latest chown -R $$(id -u):$$(id -g) target || true
	@echo "=========================================================="
	@echo "Build complete! Executable generated at: ./target/release/ikide"
	@echo "=========================================================="

# Runs the IDE locally
run:
	@if [ ! -f "./target/release/ikide" ]; then \
		echo "Executable not found. Running 'make build' first..."; \
		$(MAKE) build; \
	fi
	@echo "Starting ikide..."
	@./target/release/ikide

# Cleans compiled artifacts
clean:
	@echo "Cleaning Cargo build artifacts..."
	docker run --rm -v "$$(pwd):/volume" -w /volume rust:latest cargo clean

# Docker cross-platform builds
build-linux:
	@echo "Building for Linux via Docker..."
	docker run --rm -v "$$(pwd):/workspace" -w /workspace rust:latest bash -c "cargo build --release --target x86_64-unknown-linux-gnu && chown -R $$(id -u):$$(id -g) target || true"

build-windows:
	@echo "Building for Windows via Docker..."
	docker run --rm -v "$$(pwd):/workspace" -w /workspace rust:latest bash -c "rustup target add x86_64-pc-windows-gnu && apt-get update && apt-get install -y mingw-w64 && cargo build --release --target x86_64-pc-windows-gnu && chown -R $$(id -u):$$(id -g) target || true"

build-macos:
	@echo "Building for macOS via Docker..."
	docker run --rm -v "$$(pwd):/workspace" -w /workspace joseluisq/rust-linux-darwin-builder:latest bash -c "cargo build --release --target x86_64-apple-darwin && chown -R $$(id -u):$$(id -g) target || true"
