#!/usr/bin/env bash
# The gate, with every exit code checked (never a piped summary). Run from anywhere.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
# The default profile of .config/nextest.toml kills a hung test after 2 minutes. nextest does not
# run doctests, so those run on their own.
cargo nextest run --workspace --all-features --profile default
cargo test --workspace --all-features --doc
cargo test -p policy-point -p docket-eval
./scripts/check-boundary.sh
./scripts/check-portable.sh
cargo deny check licenses
echo "GATE GREEN"
