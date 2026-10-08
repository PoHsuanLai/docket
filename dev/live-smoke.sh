#!/usr/bin/env bash
# Live smoke: the docket-accept flows (dev/accept) against a real model instead of their cassettes,
# with every daemon real (intentd, companiond, readerd, memoryd, inferd) on a private bus.
#
#   dev/live-smoke.sh --engine scripted                       # the cassettes: proves the harness
#   dev/live-smoke.sh --engine local --inferd-config dev/live/inferd.local.toml
#   dev/live-smoke.sh --engine cloud --inferd-config dev/live/inferd.cloud.toml --accountd <bin>
#   dev/live-smoke.sh --engine local --inferd-config F --flow flow-c --patience-s 900
#   dev/live-smoke.sh --engine scripted --agent acp --acp-command ABS ...   # an external ACP agent plays the
#     companion (docs/live-eval.md, "An external agent instead of the planner"); its own network is said first;
#     add --acp-profile claude-code to confine Claude Code's connectors, skills, plugins and its own
#     permission prompt for the desktop's tools (docs/live-eval.md, FINDINGS.md "acp-isolation")
#
# Same engine flag and the same isolation as scripts/eval-release.sh: `env -i`, scratch HOME and XDG
# directories, a private bus, no real mail or memory; the network only with --engine cloud, said
# before it starts. Prints PASS or FAIL per flow with the transcript's path. A FAIL line says
# [safety] (must hold whatever the model does) or [capability] (the model did not get the job
# done). The transcript holds every request each daemon sent a model and what it said (the model
# tap, DOCKET_MODEL_TRACE), the answer's phases, the sheets and what the mail app did.
# --catalog DIR: the model catalogue copied into the scratch world (default: ../stoker/catalog, absolute;
# inferd there reads $XDG_DATA_HOME/stoker/catalog, which is scratch). Copied, never linked.
# Exit: 0 every flow passed, 1 one failed, 2 the run could not start.
set -euo pipefail
cd "$(dirname "$0")/.."
root=$(pwd)

engine=scripted
catalog=$(cd "$root/../stoker/catalog" 2>/dev/null && pwd || true)
args=()
while [ $# -gt 0 ]; do
  case "$1" in
    --engine) engine=$2; args+=("$1" "$2"); shift 2 ;;
    --catalog) catalog=$2; shift 2 ;;
    *) args+=("$1"); shift ;;
  esac
done
case "$engine" in
  scripted) echo "live-smoke: --engine scripted: no network, no model (cassettes)" >&2 ;;
  local)    echo "live-smoke: --engine local: no network; your inferd config's local engines run on this machine" >&2 ;;
  cloud)    echo "live-smoke: --engine cloud: NETWORK. Prompts, including a mail thread's text given to the reader, go to the hosted models your inferd config names. Ctrl-C now to stop." >&2; sleep 5 ;;
  *) echo "live-smoke: --engine takes scripted, local or cloud" >&2; exit 2 ;;
esac

export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$(dirname "$root")/accept-target}
cargo build -p docket-accept --bins
bin=$CARGO_TARGET_DIR/debug/docket-live

scratch=${LIVE_SCRATCH:-$(mktemp -d "${TMPDIR:-/tmp}/docket-live.XXXXXX")}
mkdir -p "$scratch/home" "$scratch/tmp" "$scratch/run"
chmod 700 "$scratch/run"
# vLLM, Triton and Inductor compile caches, kept across worlds and runs (a cold compile is
# minutes; every world starts a fresh inferd). Engine build output only, never model input.
engine_cache=${ENGINE_CACHE:-${TMPDIR:-/tmp}/docket-live-engine-cache}
mkdir -p "$engine_cache"
echo "live-smoke: scratch $scratch" >&2
exec env -i PATH="$PATH" HOME="$scratch/home" TMPDIR="$scratch/tmp" XDG_RUNTIME_DIR="$scratch/run" \
  DOCKET_LIVE_ENGINE_CACHE="$engine_cache" \
  XDG_CONFIG_HOME="$scratch/home/.config" XDG_DATA_HOME="$scratch/home/.local/share" \
  XDG_STATE_HOME="$scratch/home/.local/state" XDG_CACHE_HOME="$scratch/home/.cache" \
  "$bin" smoke --out "$scratch/out" ${catalog:+--catalog "$catalog"} "${args[@]}"
