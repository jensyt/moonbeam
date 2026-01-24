#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}============================================================${NC}"
    echo -e "${BLUE}Running: $@${NC}"
    echo -e "${BLUE}============================================================${NC}"
}

run_check() {
    print_header "cargo clippy $* -- -D warnings"
    cargo clippy "$@" -- -D warnings

    if [[ "$*" == *"--all-features"* ]]; then
        print_header "cargo test $*"
        cargo test "$@"
    else
        print_header "cargo test $* --lib --bins --tests"
        cargo test "$@" --lib --bins --tests
    fi
}

echo -e "${GREEN}Starting comprehensive check suite...${NC}"

# 1. No default features (Baseline)
run_check --workspace --no-default-features

# 2. All features (Full coverage)
run_check --workspace --all-features

# 3. Individual features (Moonbeam)
# We test these one by one to ensure no implicit dependencies are broken
FEATURES=(
    "assets"
    "macros"
    "catchpanic"
    "signals"
    "tracing"
    "compress"
    "router"
    "mt"
)

for feature in "${FEATURES[@]}"; do
    run_check --workspace --no-default-features --features "$feature"
done

# 4. Logical Feature Groups

# Multi-threaded server with graceful shutdown signals
run_check --workspace --no-default-features --features "mt,signals"

# Multi-threaded server with tracing enabled (common production setup)
run_check --workspace --no-default-features --features "mt,tracing"

# Router with compression (common web server setup)
run_check --workspace --no-default-features --features "router,compress"

# Assets serving with compression
run_check --workspace --no-default-features --features "assets,compress"

# 5. Moonbeam Attributes specific checks
# Although covered by workspace checks mostly, explicit checks ensure the macro crate stands alone correctly
print_header "Checking moonbeam-attributes explicitly"
cargo clippy -p moonbeam-attributes --no-default-features -- -D warnings
cargo clippy -p moonbeam-attributes --no-default-features --features router -- -D warnings
cargo clippy -p moonbeam-attributes --no-default-features --features autohead -- -D warnings
