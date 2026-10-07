#!/usr/bin/env bash
# The release eval: plays every corpus of eval/ through the router with the REAL policy writer and
# the REAL reviewer cascade (intentd's and action-review's code) asking a real inferd, over
# docket-fake's providers, and writes eval/reports/<label>.md. A dev script, not part of the gate.
#
#   scripts/eval-release.sh --engine scripted
#   scripts/eval-release.sh --engine local --inferd-config dev/live/inferd.local.toml
#   scripts/eval-release.sh --engine cloud --inferd-config dev/live/inferd.cloud.toml --accountd <bin>
#
# The model source is a flag. `scripted` plays a cassette (the gate's hijacked judge): no model, no
# network. `local` is inferd's configured local engines (the inferd.toml you name). `cloud` is
# inferd with network and a hosted model named by catalogue id in that file; the key is accountd's
# (docs/live-eval.md says how it gets there). The script reads no key, from no variable and no file.
#
# What it does, and what it never does:
#   1. builds docket-live (and the siblings it runs: inferd, memoryd) from this checkout;
#   2. runs it under `env -i`, with a scratch HOME and XDG directories and no session bus
#      address, so nothing touches ~/.config, the real bus, the real mail or memory; docket-live
#      starts its own private dbus-daemon and inferd;
#   3. for every case: plays the turns, the scripted planner steps and the world through the
#      router, with the real writer and cascade; per corpus: FPR and FNR with Wilson intervals,
#      ask and approve rates, latency per stage, spend (docket_eval::RunReport); a transcript, a
#      cassette and a case file per case under <scratch>/traces;
#   4. the network is touched only with --engine cloud, and it says so before starting.
#
# The person's FNR target (QUESTIONS S5; only they may raise it) is --fnr-max-permille N: a
# corpus whose false-negative rate has a Wilson upper end above it fails the run. Shrinking the
# AllowJudged cells to Ask (a default.cedar change) until it passes is NOT automated: read the
# report and the traces, then change the policy by hand.
#
# --catalog DIR: the model catalogue copied into the scratch world (default: ../stoker/catalog, absolute;
# inferd there reads $XDG_DATA_HOME/stoker/catalog, which is scratch). Copied, never linked.
# Exit: 0 every case met what it expected (and every target held), 1 not, 2 the run could not start.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

engine=scripted
catalog=$(cd "$root/../stoker/catalog" 2>/dev/null && pwd || true)
label=
extra=()
while [ $# -gt 0 ]; do
  case "$1" in
    --engine) engine=$2; extra+=("$1" "$2"); shift 2 ;;
    --label) label=$2; shift 2 ;;
    --catalog) catalog=$2; shift 2 ;;
    *) extra+=("$1"); shift ;;
  esac
done
[ -n "$label" ] || label="$(date -u +%Y%m%d-%H%M)-$engine"

case "$engine" in
  scripted) echo "eval-release: --engine scripted: no network, no model (a cassette)" >&2 ;;
  local)    echo "eval-release: --engine local: no network; your inferd config's local engines run on this machine" >&2 ;;
  cloud)    echo "eval-release: --engine cloud: NETWORK. Prompts (the person's words, the stripped review requests, the action catalogue; never mail bodies) go to the hosted models your inferd config names. Ctrl-C now to stop." >&2; sleep 5 ;;
  *) echo "eval-release: --engine takes scripted, local or cloud" >&2; exit 2 ;;
esac

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(dirname "$root")/accept-target}
cargo build -p docket-accept --bins
bin=$CARGO_TARGET_DIR/debug/docket-live

scratch=${LIVE_SCRATCH:-$(mktemp -d "${TMPDIR:-/tmp}/docket-live.XXXXXX")}
mkdir -p "$scratch/home" "$scratch/tmp" "$scratch/run"
chmod 700 "$scratch/run"
report=$root/eval/reports/$label.md
echo "eval-release: scratch $scratch; report $report" >&2

set +e
env -i PATH="$PATH" HOME="$scratch/home" TMPDIR="$scratch/tmp" XDG_RUNTIME_DIR="$scratch/run" \
  XDG_CONFIG_HOME="$scratch/home/.config" XDG_DATA_HOME="$scratch/home/.local/share" \
  XDG_STATE_HOME="$scratch/home/.local/state" XDG_CACHE_HOME="$scratch/home/.cache" \
  "$bin" corpus --label "$label" --out "$scratch/out" --report "$report" \
  --eval-dir "$root/eval" ${catalog:+--catalog "$catalog"} "${extra[@]}"
status=$?
set -e
echo "eval-release: traces: $scratch/out/traces/index.txt" >&2
exit $status
