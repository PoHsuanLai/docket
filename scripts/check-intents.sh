#!/usr/bin/env bash
# The conformance check of cli.md section 6, for an app repository: "every app we ship can be
# driven from a command line" holds by construction. Copy this file to an app repo's scripts/ and
# run it from the app's gate. It fails when
#   - the app ships a .desktop file and no intents manifest,
#   - a manifest does not validate (or an action has no parameter schema),
#   - a menu command or shortcut in <AppName>.ui.toml names no action and is not UI-only.
#
# usage: scripts/check-intents.sh [repository dir]       (default: the repository this file is in)
# environment:
#   DOCKET_EVAL  a built docket-eval binary to run instead of building one
#   DOCKET_DIR   the docket checkout to build it from (default: ../docket beside the repository)
set -euo pipefail
repo=${1:-$(cd "$(dirname "$0")/.." && pwd)}
if [ -n "${DOCKET_EVAL:-}" ]; then
  exec "$DOCKET_EVAL" --check-app "$repo"
fi
docket=${DOCKET_DIR:-$repo/../docket}
if [ ! -f "$docket/Cargo.toml" ]; then
  echo "check-intents: no docket checkout at $docket (set DOCKET_DIR, or DOCKET_EVAL to a built docket-eval)" >&2
  exit 2
fi
exec cargo run --quiet --manifest-path "$docket/Cargo.toml" -p docket-eval --bin docket-eval -- --check-app "$repo"
