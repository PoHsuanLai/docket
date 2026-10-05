#!/usr/bin/env bash
# The agent tier's end-to-end acceptance, run on its own inside the jail.
#
#   dev/accept/run.sh [extra nextest args, e.g. a test name filter]
#
# This is the same test the gate runs (`crates/docket-accept/tests/flows.rs`, part of
# `cargo nextest run --workspace` under `~/rs-wt/gate-lane-jailed.sh`); this script only runs it
# alone, for working on it. The build happens outside the jail, the test archive runs inside
# ~/desktop/harness/jail.sh: no /dev/input, no /dev/dri, no network, a scratch HOME. The test
# itself makes a private dbus-daemon, kills every process it starts by PID, and names nothing of
# the real session.
set -euo pipefail
here=$(cd "$(dirname "$0")/../.." && pwd)
cd "$here"
target=${CARGO_TARGET_DIR:-$(dirname "$here")/accept-target}
export CARGO_TARGET_DIR=$target
jail=${JAIL:-$HOME/desktop/harness/jail.sh}
archive=$target.accept.tar.zst

cargo nextest archive -p docket-accept --all-features --archive-file "$archive"
"$jail" --bind "$here" --bind "$target" -- \
  cargo-nextest nextest run --archive-file "$archive" --workspace-remap "$here" \
  --no-capture -E 'package(docket-accept)' "$@"
