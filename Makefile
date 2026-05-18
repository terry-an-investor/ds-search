.PHONY: build check test sweep clean

# Build and clean intermediate artifacts (keeps final binary only)
build:
	cargo build
	cargo sweep --time 0

# Check without cleaning (fast, for iteration)
check:
	cargo check

# Run tests
test:
	cargo test
	cargo sweep --time 0

# Sweep build artifacts older than 1 day (safe incremental)
sweep:
	cargo sweep --time 1

# Full clean
clean:
	cargo clean
