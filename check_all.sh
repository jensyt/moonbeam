#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_header() {
    echo -e "\n${BLUE}============================================================${NC}"
    echo -e "${BLUE}$@${NC}"
    echo -e "${BLUE}============================================================${NC}"
}

run_check() {
    # Check moonbeam library
    print_header "Checking 'moonbeam' library: cargo clippy -p moonbeam $@"
    cargo clippy -p moonbeam "$@" -- -D warnings

    # Run library unit tests
    if [[ "$*" == *"--all-features"* ]]; then
         cargo test -p moonbeam "$@"
    else
         # Run only lib tests (unit tests)
         cargo test -p moonbeam "$@" --lib
    fi

    # Check integration tests (features mapped 1:1)
    print_header "Checking 'tests-integration': cargo clippy -p tests-integration $@"
    cargo clippy -p tests-integration "$@" -- -D warnings

    # Run integration tests
    cargo test -p tests-integration "$@"
}

echo -e "${GREEN}Starting comprehensive check suite...${NC}"

# 1. Check Examples (Standalone checks to ensure they build)
print_header "Checking Examples"
print_header "examples-basic"
cargo check -p examples-basic
print_header "examples-routing"
cargo check -p examples-routing
print_header "examples-concurrent"
cargo check -p examples-concurrent

# 2. No default features (Baseline)
run_check --no-default-features

# 3. All features (Full coverage)
run_check --all-features

# 4. Individual features (Moonbeam)
FEATURES=(
    "assets"
    "macros"
    "catchpanic"
    "signals"
    "tracing"
    "compress"
    "router"
    "mt"
    "disable-simd"
)

for feature in "${FEATURES[@]}"; do
    run_check --no-default-features --features "$feature"
done

# 5. Logical Feature Groups
run_check --no-default-features --features "mt,signals"
run_check --no-default-features --features "mt,tracing"
run_check --no-default-features --features "router,compress"
run_check --no-default-features --features "assets,compress"

# 6. Moonbeam Attributes specific checks
print_header "Checking moonbeam-attributes explicitly"
cargo clippy -p moonbeam-attributes --no-default-features -- -D warnings
cargo clippy -p moonbeam-attributes --no-default-features --features router -- -D warnings
cargo clippy -p moonbeam-attributes --no-default-features --features autohead -- -D warnings

echo
echo -e "${GREEN}All tests passed.${NC}"
