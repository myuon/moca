.PHONY: check fix build test clean fmt clippy

# Run all checks (cargo check, clippy, fmt check, test)
check: cargo-check clippy fmt-check test

# Fix lints and format code
fix:
	cargo fix --allow-dirty --allow-staged
	cargo clippy --fix --allow-dirty --allow-staged
	cargo fmt

# Individual targets
cargo-check:
	cargo check

clippy:
	cargo clippy -- -D warnings

fmt-check:
	cargo fmt -- --check

fmt:
	cargo fmt

test:
	cargo test

build:
	cargo build

build-release:
	cargo build --release

clean:
	cargo clean

# Run the project
run:
	cargo run

# Help target
help:
	@echo "Available targets:"
	@echo "  check        - Run all checks (cargo check, clippy, fmt check, test)"
	@echo "  fix          - Fix lints and format code"
	@echo "  cargo-check  - Run cargo check"
	@echo "  clippy       - Run clippy with warnings as errors"
	@echo "  fmt-check    - Check formatting without modifying files"
	@echo "  fmt          - Format code"
	@echo "  test         - Run tests"
	@echo "  build        - Build debug version"
	@echo "  build-release- Build release version"
	@echo "  clean        - Clean build artifacts"
	@echo "  run          - Run the project"
