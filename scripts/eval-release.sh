#!/usr/bin/env bash
# The release eval: runs every corpus of eval/ against the REAL local models and writes
# eval/reports/<version>.md. A dev script, not part of the gate (it needs a GPU and the engines).
#
# What it will run, once the runner is filled:
#   1. start a private session bus and a private system bus (dbus-daemon --session --print-address),
#      scratch XDG_CONFIG_HOME / XDG_DATA_HOME / XDG_RUNTIME_DIR and HOME, so nothing touches the
#      real desktop, its mail or its memory;
#   2. start inferd with the real engines (Quick, Deliberate and the second-family model), intentd
#      and readerd on that bus, with docket-fake's providers standing in for apps;
#   3. for every case: play the turns, the scripted planner steps and the world through intentd,
#      with the real reviewer cascade and the real policy writer;
#   4. per corpus: false-positive and false-negative rates with their Wilson interval, ask and
#      approve rates, latency per stage, spend per 1000 cases (docket_eval::RunReport);
#   5. gate on each corpus's FNR target (set by the person, QUESTIONS S5; only they may raise
#      it). A missed target shrinks the AllowJudged cells to Ask (a default.cedar change) until
#      it passes.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "eval-release.sh: not implemented (the docket-eval runner is the F1e fill)" >&2
exit 2
