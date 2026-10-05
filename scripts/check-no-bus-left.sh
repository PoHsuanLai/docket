#!/usr/bin/env bash
# Run a crate's tests, then check that no private dbus-daemon of theirs is left running.
#
# Every private bus is made in a tempfile directory, and its `dbus-daemon` command line names that
# directory (`--config-file=<dir>/bus.conf`). This script points TMPDIR at a scratch directory of
# its own, runs the tests, and then looks, by reading /proc, for a live dbus-daemon whose command
# line contains that scratch directory. Nothing is matched by name to be killed: a leftover is
# reported by PID and ended by that PID alone. The real session and system buses are never named.
#
# usage: scripts/check-no-bus-left.sh [cargo test args]   (default: -p readerd)
set -uo pipefail
cd "$(dirname "$0")/.."

scratch=$(mktemp -d "${TMPDIR:-/tmp}/bus-leak.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
args=("$@")
[ "${#args[@]}" -gt 0 ] || args=(-p readerd)

TMPDIR="$scratch" cargo test "${args[@]}"
tests=$?

left=()
for dir in /proc/[0-9]*; do
  pid=${dir#/proc/}
  # A process can exit between the glob and the read; that is not an error.
  cmd=$({ tr '\0' ' ' <"$dir/cmdline"; } 2>/dev/null) || continue
  state=$({ awk '{print $3}' "$dir/stat"; } 2>/dev/null) || continue
  [ "$state" = Z ] && continue
  case "$cmd" in
    *dbus-daemon*"$scratch"*)
      left+=("$pid")
      echo "LEAK: dbus-daemon pid $pid outlived the tests: $cmd"
      ;;
  esac
done

if [ "${#left[@]}" -gt 0 ]; then
  for pid in "${left[@]}"; do
    kill "$pid" 2>/dev/null || true
  done
  exit 1
fi
echo "NO BUS LEFT: no dbus-daemon names $scratch (tests exit code $tests)"
exit "$tests"
