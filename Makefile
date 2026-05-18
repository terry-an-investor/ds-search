.PHONY: build check test sweep clean clean-system

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
	cargo sweep --time 0

# Full project clean
clean:
	cargo clean

# System-wide clean — removes stale crate sources from ~/.cargo
clean-system:
	cargo cache --autoclean
