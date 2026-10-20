default:
    @just --list

# Format all files with treefmt (nixfmt + rustfmt)
fmt:
    nix fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Run clippy and treat warnings as errors
lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-features

run *ARGS:
    cargo run -- {{ARGS}}

# Run every check that CI runs
check: fmt-check lint test
