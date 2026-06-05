#!/bin/bash
set -e

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Temporary file for logs
LOG_FILE=$(mktemp)
trap "rm -f $LOG_FILE" EXIT

print_header() {
    echo -e "\n${BLUE}=== $@ ===${NC}"
}

run_logged() {
    local desc="$1"
    shift
    printf "%-80s " "$desc"
    if "$@" > "$LOG_FILE" 2>&1; then
        echo -e "${GREEN}OK${NC}"
    else
        echo -e "${RED}FAILED${NC}"
        echo -e "\n--- Log Output ---"
        cat "$LOG_FILE"
        echo -e "------------------\n"
        exit 1
    fi
}

run_check() {
    local features="$@"
    local feat_desc="${features:-no-default-features}"

    # Check moonbeam library
    run_logged "moonbeam (clippy) [$feat_desc]" \
        cargo clippy -p moonbeam $features -- -D warnings

    # Run library unit tests
    if [[ "$features" == *"--all-features"* ]]; then
         run_logged "moonbeam (test) [$feat_desc]" \
            cargo test -p moonbeam $features
    else
         # Run only lib tests (unit tests)
         run_logged "moonbeam (test-lib) [$feat_desc]" \
            cargo test -p moonbeam $features --lib
    fi

    # Check integration tests (features mapped 1:1)
    run_logged "tests-integration (clippy) [$feat_desc]" \
        cargo clippy -p tests-integration $features -- -D warnings

    # Run integration tests
    run_logged "tests-integration (test) [$feat_desc]" \
        cargo test -p tests-integration $features
}

echo -e "${GREEN}Starting comprehensive check suite...${NC}"

# 1. Check Examples
print_header "Examples"
EXAMPLES=(
    "examples-basic"
    "examples-routing"
    "examples-concurrent"
    "examples-tls"
    "examples-tracing"
)

for example in "${EXAMPLES[@]}"; do
    run_logged "check $example" cargo check -p "$example"
done

# 2. Check moonbeam-serde and moonbeam-forms
print_header "Support Crates"
run_logged "moonbeam-serde (clippy)" cargo clippy -p moonbeam-serde -- -D warnings
run_logged "moonbeam-serde (test)" cargo test -p moonbeam-serde
run_logged "moonbeam-forms (clippy)" cargo clippy -p moonbeam-forms -- -D warnings
run_logged "moonbeam-forms (test)" cargo test -p moonbeam-forms

# 3. No default features (Baseline)
print_header "Baseline (No Features)"
run_check --no-default-features

# 4. All features (Full coverage)
print_header "Full Coverage (All Features)"
run_check --all-features

# 5. Individual features (Moonbeam)
print_header "Individual Features"
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
    "tls"
)

for feature in "${FEATURES[@]}"; do
    run_check --no-default-features --features "$feature"
done

# 6. Logical Feature Groups
print_header "Feature Groups"
run_check --no-default-features --features "mt,signals"
run_check --no-default-features --features "mt,tracing"
run_check --no-default-features --features "router,compress"
run_check --no-default-features --features "assets,compress"
run_check --no-default-features --features "mt,tls"
run_check --no-default-features --features "tls,signals"

# 7. Moonbeam Attributes specific checks
print_header "Moonbeam Attributes"
run_logged "moonbeam-attributes (no-features)" \
    cargo clippy -p moonbeam-attributes --no-default-features -- -D warnings
run_logged "moonbeam-attributes (router)" \
    cargo clippy -p moonbeam-attributes --no-default-features --features router -- -D warnings
run_logged "moonbeam-attributes (autohead)" \
    cargo clippy -p moonbeam-attributes --no-default-features --features autohead -- -D warnings

echo
echo -e "${GREEN}All tests passed.${NC}"
