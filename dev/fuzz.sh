#!/usr/bin/env bash
# Fuzz docket's parsers of model and peer output with cargo-fuzz (libFuzzer). Not in the gate: it
# needs a nightly toolchain and `cargo-fuzz`, and it runs for as long as you give it.
#
#   dev/fuzz.sh                      # list the targets
#   dev/fuzz.sh parse_verdict        # fuzz one target for 60 seconds
#   dev/fuzz.sh all 300              # every target, 300 seconds each
#
# Targets (fuzz/fuzz_targets): parse_verdict (a reviewer's reply; never an allow but for the exact
# shapes), tool_args (a tool call's arguments read by a real action's types), unframe (voice-wire's
# event fd), companion_wire (Companion1 bodies), leaked_call (a call written as words), case_files
# (corpus and planner-case TOML).
#
# This script installs nothing. If the nightly toolchain or cargo-fuzz is missing it says so and
# exits 2: install nightly with rustup, and cargo-fuzz with `cargo install cargo-fuzz`.
# Crashes land in fuzz/artifacts/<target>/; a crashing input becomes a unit or property test.
set -euo pipefail
cd "$(dirname "$0")/.."

targets=(parse_verdict tool_args unframe companion_wire leaked_call case_files)
if [ $# -eq 0 ]; then
  printf 'targets:\n'
  printf '  %s\n' "${targets[@]}"
  exit 0
fi
if ! rustup toolchain list 2>/dev/null | grep -q '^nightly'; then
  echo "fuzz: no nightly toolchain installed (rustup toolchain install nightly); not installing it" >&2
  exit 2
fi
nightly=(cargo "+nightly")
if ! "${nightly[@]}" fuzz --version >/dev/null 2>&1; then
  echo "fuzz: cargo-fuzz is not installed (cargo install cargo-fuzz); not installing it" >&2
  exit 2
fi
seconds=${2:-60}
run=("$1")
[ "$1" != all ] || run=("${targets[@]}")
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(dirname "$(pwd)")/fuzz-target}
for target in "${run[@]}"; do
  echo "fuzz: $target for ${seconds}s" >&2
  (cd fuzz && "${nightly[@]}" fuzz run "$target" -- -max_total_time="$seconds")
done
