#!/usr/bin/env bash
# Crate layers, mechanically enforced (ARCHITECTURE.md section 1 is the table; this is its
# mechanical form).
#
# Three kinds of check:
#   1. LAYERS (below): every crate is P (pure), C (client/IO) or A (app); dependencies point down;
#      each tier has source bans; KNOWN lists the breaches that still stand and the lane that
#      removes each. `--self-test` seeds two violations into a copy and asserts they fail.
#   2. RULES: what a crate reaches through ANY path (cargo tree), the effect crates it must not.
#   3. Repo-wide: nothing of cua or sill, no test switch in a shipped crate.
#
# `cargo tree -i <dep>` exits 101 when the dependency is absent, which is precisely the state
# we want. Checking the exit status would therefore fail whenever the boundary holds, so we
# check for OUTPUT instead: any line naming the dependency is a leak.
set -uo pipefail
cd "$(dirname "$0")/.."

# ---------------------------------------------------------------------------------------------
# LAYERS: every workspace crate is one tier (agent-layers plan, X2). Dependencies point down.
#   P  pure: no filesystem, environment, process, network, clock read, sleep, tokio, bus or
#      desktop library. Logic over values and seams.
#   C  client / IO: may do IO, carries no app or UI concepts (no labels, copy, rows or sheets).
#   A  app / daemon / test binary: wiring only; its logic lives in P and C libraries.
# The row is the tier the crate SHOULD have. A crate that breaks its tier today is listed in
# KNOWN below with the plan lane that removes the breach; there is no other allowance.
LAYERS=(
  # pure
  "docket-core:P"
  "docket-skills:P"
  "policy-point:P"
  "action-review:P"
  "docket-router:P"
  "companion-wire:P"
  "agent-loop:P"
  "docket-session:P"
  "docket-checkpoint:P"
  "actions-tools:P"
  "voice-wire:P"
  "voice-loop:P"
  # client / IO
  "docket-planner:C"
  "docket-tasks:C"
  "docket-memory:C"
  "docket-models:C"
  "docket-reader:C"
  "docket-fake:C"
  "docket-checkpoint-git:C"
  "docket-acp:C"
  "docket-launch:C"
  "docket-agents:C"
  "docket-settings:C"
  "docket-dbus:C"
  "docket-client:C"
  "companion-client:C"
  "voice-dbus:C"
  "docket-log:C"
  # app / daemon / test binary
  "docket-acp-bin:A"
  "docket-inapp:A"
  "docket-eval:A"
  "docket-testbus:A"
  "docket-accept:A"
  "actions-mcp:A"
  "docket-cli:A"
  "docket-ds:A"
  "intentd:A"
  "companiond:A"
  "readerd:A"
  "voiced:A"
)

# KNOWN: the only allowance. One row per breach: "<crate> <key> <lane that removes it>".
# Keys: dep:<crate> (a dependency that points up or is banned), src:<pattern> (a banned source
# pattern), ui:<word>, env (an environment read in an app library), reexport:glob and
# reexport:ext. The list only shrinks: each lane deletes its rows, and a row that no longer
# matches a breach fails the check until it is deleted.
KNOWN=(
  "docket-acp ui:Sheet D0.16"
  "docket-tasks ui:Sheet D0.13"
  "docket-core src:std::fs D0.7"
  "docket-core src:thread::sleep D0.7"
  "docket-skills src:std::fs D0.24"
  "actions-mcp env D0.4"
  "docket-acp-bin env D0.21"
  "docket-accept env D0.22"
  "companiond env D0.25"
  "intentd env D0.25"
  "readerd env D0.25"
)

# Files over 500 lines are reported. F.1 turns the report into a failure: set this to "yes".
FAIL_ON_LONG=no
LONG_LINES=500

# External crates a P-tier crate may not depend on (regex on the dependency name).
P_BANNED_DEPS='^(tokio|zbus|rustix|nix|hyper|reqwest|quire(-.*)?|ds-.*|.*-dbus)$'
# Source patterns banned in P-tier src/: "key|regex".
P_BANNED_SRC=(
  'std::fs|\bstd::fs\b|\bstd::\{[^}]*\bfs\b'
  'std::env|\bstd::env\b|\bstd::\{[^}]*\benv\b'
  'std::process|\bstd::process\b|\bstd::\{[^}]*\bprocess\b'
  'std::net|\bstd::net\b|\bstd::\{[^}]*\bnet\b'
  'SystemTime::now|\bSystemTime::now\b'
  'Instant::now|\bInstant::now\b'
  'thread::sleep|\bthread::sleep\b'
  'tokio::|\btokio::'
)
# UI words banned, as identifiers, in C-tier src/ (each is matched as a whole word).
C_BANNED_UI=(Label Help Sheet SettingRow ds_settings)
# Environment reads banned in A-tier library code (everything but main.rs and src/bin/).
A_BANNED_ENV='\benv::(var|var_os|vars|vars_os)\b'

declare -A TIER=()
for row in "${LAYERS[@]}"; do TIER["${row%%:*}"]="${row#*:}"; done
rank() { case "$1" in P) echo 0 ;; C) echo 1 ;; A) echo 2 ;; esac; }

declare -A FOUND=()
# viol <crate> <key> <detail>: record one breach (the first lines of detail are shown).
viol() { FOUND["$1 $2"]+="$3"$'\n'; }

declare -A DIR_NAME=() DEPS=() TIER_DIRS=()

# scan_tier <tier> <key> <regex> [libs]: lines of src/**/*.rs in every crate of that tier that
# match <regex>, one rg run for the whole tier. Comment lines and `#[tokio::test]` attributes (a
# dev-dependency) are dropped; with "libs", main.rs and src/bin/ are left out too.
scan_tier() {
  local tier=$1 key=$2 regex=$3 only=${4:-} dirs=() d hits line dir
  for d in ${TIER_DIRS[$tier]:-}; do dirs+=("$d/src"); done
  [ "${#dirs[@]}" -gt 0 ] || return 0
  hits=$(rg -n --no-heading -g '*.rs' -e "$regex" "${dirs[@]}" 2>/dev/null \
    | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//|#\[tokio::test' || true)
  [ "$only" = libs ] && hits=$(grep -vE '/src/main\.rs:|/src/bin/' <<<"$hits" || true)
  # prov's `Label` is the provenance label on data, not a label a person reads: a file that
  # imports it from prov is not a use of the UI word.
  if [ "$key" = ui:Label ] && [ -n "$hits" ]; then
    while IFS= read -r line; do
      hits=$(grep -vF "$line:" <<<"$hits" || true)
    done < <(rg -l -U -g '*.rs' 'prov::(\{[^}]*\bLabel\b|Label\b)' "${dirs[@]}" 2>/dev/null)
  fi
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    dir=${line%%:*}; dir=${dir%%/src/*}
    viol "${DIR_NAME[$dir]}" "$key" "$line"
  done <<<"$hits"
}

layers_check() {
  local root=$1 meta name dir deps tier d dt spec key regex word hits f n line seg
  command -v rg >/dev/null || { echo "ERROR: rg is needed for the source checks"; return 1; }
  command -v jq >/dev/null || { echo "ERROR: jq is needed to read cargo metadata"; return 1; }
  meta=$(cd "$root" && cargo metadata --no-deps --offline --format-version 1) \
    || { echo "ERROR: cargo metadata failed; the layers were not checked"; return 1; }
  FOUND=(); DIR_NAME=(); DEPS=(); TIER_DIRS=()
  local members=() alldirs=()
  # (a) dependencies point down; P crates avoid the banned externals
  while IFS=$'\t' read -r name dir deps; do
    members+=("$name"); alldirs+=("$dir/src"); DIR_NAME["$dir"]=$name; DEPS["$name"]=$deps
    tier=${TIER[$name]:-}
    if [ -z "$tier" ]; then
      viol "$name" missing-layer "$name has no row in LAYERS"
      continue
    fi
    TIER_DIRS["$tier"]+="$dir "
    for d in $deps; do
      dt=${TIER[$d]:-}
      if [ -n "$dt" ]; then
        [ "$(rank "$dt")" -gt "$(rank "$tier")" ] \
          && viol "$name" "dep:$d" "$tier crate depends on $dt crate $d (dependencies point down)"
      elif [ "$tier" = P ] && [[ $d =~ $P_BANNED_DEPS ]]; then
        viol "$name" "dep:$d" "P crate depends on $d"
      fi
    done
  done < <(jq -r '.packages[] | [.name, (.manifest_path | sub("/Cargo.toml$"; "")),
      ([.dependencies[] | select(.kind != "dev") | .name] | unique | join(" "))] | @tsv' <<<"$meta")

  # (b) P-tier source bans, (c) C-tier UI words, (d) A-tier environment reads
  for spec in "${P_BANNED_SRC[@]}"; do
    scan_tier P "src:${spec%%|*}" "${spec#*|}"
  done
  for word in "${C_BANNED_UI[@]}"; do scan_tier C "ui:$word" "\\b$word\\b"; done
  scan_tier A env "$A_BANNED_ENV" libs

  # (e) long files: reported; a failure once FAIL_ON_LONG is yes
  while read -r n f; do
    [ "$f" = total ] && continue
    [ "$n" -gt "$LONG_LINES" ] || continue
    if [ "$FAIL_ON_LONG" = yes ]; then
      dir=${f%%/src/*}
      [ -n "${DIR_NAME[$dir]:-}" ] && viol "${DIR_NAME[$dir]}" "long:${f#"$dir"/}" "$f has $n lines"
    else
      echo "long file (report only): ${f#"$root"/} has $n lines"
    fi
  done < <(find "${alldirs[@]}" -name '*.rs' -print0 2>/dev/null | xargs -0 wc -l 2>/dev/null)

  # (f) re-exports: a glob, or a whole external crate or module (a lowercase path end)
  hits=$(rg -n --no-heading -g '*.rs' -e '^\s*pub(\([a-z: ]+\))?\s+use\s' "${alldirs[@]}" 2>/dev/null || true)
  while IFS= read -r line; do
    [ -z "$line" ] && continue
    dir=${line%%:*}; dir=${dir%%/src/*}
    name=${DIR_NAME[$dir]:-}
    [ -n "$name" ] || continue
    if [[ $line =~ ::\*[[:space:]]*\; ]]; then
      viol "$name" reexport:glob "$line"; continue
    fi
    [[ $line == *'{'* ]] && continue
    [[ $line =~ pub(\([a-z:\ ]+\))?[[:space:]]+use[[:space:]]+([A-Za-z0-9_]+) ]] || continue
    seg=${BASH_REMATCH[2]}
    # external: a dependency of this crate that is not a workspace member
    [[ " ${DEPS[$name]} " == *" ${seg//_/-} "* || " ${DEPS[$name]} " == *" $seg "* ]] || continue
    [ -n "${TIER[${seg//_/-}]:-}" ] && continue
    if [[ $line =~ use[[:space:]]+[A-Za-z0-9_]+[[:space:]]*\; ]] \
      || [[ $line =~ ::[a-z][a-z0-9_]*[[:space:]]*\; ]]; then
      viol "$name" reexport:ext "$line"
    fi
  done <<<"$hits"

  # (g) a LAYERS row for a crate that is not a workspace member is dead weight
  for name in "${!TIER[@]}"; do
    printf '%s\n' "${members[@]}" | grep -qx "$name" || viol "$name" stale-layer "LAYERS names $name, which is not a workspace member"
  done

  # Compare with KNOWN.
  local bad=0 row crate lane
  declare -A used=()
  while IFS= read -r -d '' key; do
    row=""
    for k in "${KNOWN[@]}"; do
      [ "${k% *}" = "$key" ] && row=$k
    done
    if [ -n "$row" ]; then
      used["$row"]=1
      echo "known (${row##* }): $key"
    else
      echo "LAYER: $key"
      printf '%s' "${FOUND[$key]}" | head -5 | sed 's/^/    /'
      bad=1
    fi
  done < <(printf '%s\0' "${!FOUND[@]}" | sort -z)
  for row in "${KNOWN[@]}"; do
    [ -n "${used[$row]:-}" ] || { echo "STALE: KNOWN row '$row' matches no breach; delete it"; bad=1; }
  done
  [ "$bad" -eq 0 ] && echo "layers hold: ${#members[@]} crates, dependencies point down, tier bans clear"
  return "$bad"
}

# self_test: copy the manifests and sources to a temp dir, check the copy passes, then seed one
# violation at a time (restoring the file after each) and check each fails with the expected line.
# Metadata and rg only, in one directory so cargo's work is done once.
self_test() {
  local tmp out rc=0
  tmp=$(mktemp -d) || return 1
  trap 'rm -rf "$tmp"' RETURN
  cp -r Cargo.toml Cargo.lock rust-toolchain.toml crates "$tmp/" || return 1
  [ -d .cargo ] && cp -r .cargo "$tmp/"
  out=$(layers_check "$tmp") || { echo "self-test: the unmodified copy must pass"; echo "$out"; return 1; }
  # expect <seeded file> <text appended> <expected output line> <what>
  expect() {
    cp "$tmp/$1" "$tmp/seed.bak"
    printf '%b' "$2" >>"$tmp/$1"
    out=$(layers_check "$tmp") && { echo "self-test: $4 was not caught"; rc=1; }
    grep -q "^LAYER: $3" <<<"$out" || { echo "self-test: no '$3' line for: $4"; rc=1; }
    cp "$tmp/seed.bak" "$tmp/$1"
  }
  expect crates/voice-wire/src/lib.rs '\npub fn seeded() { let _ = std::fs::read("x"); }\n' \
    'voice-wire src:std::fs' 'std::fs in a P crate'
  expect crates/docket-log/Cargo.toml '\n[dependencies.intentd]\npath = "../intentd"\n' \
    'docket-log dep:intentd' 'a C crate depending on an A crate'
  expect crates/actions-tools/src/lib.rs '\npub use serde_json;\n' \
    'actions-tools reexport:ext' 'a whole external crate re-exported'
  expect crates/actions-tools/src/lib.rs '\npub use std::collections::*;\n' \
    'actions-tools reexport:glob' 'a glob re-export'
  # A crate missing from LAYERS.
  mkdir -p "$tmp/crates/seeded/src" && : >"$tmp/crates/seeded/src/lib.rs"
  printf '[package]\nname = "seeded"\nversion = "0.0.0"\nedition = "2024"\n' >"$tmp/crates/seeded/Cargo.toml"
  cp "$tmp/Cargo.toml" "$tmp/seed.bak"
  sed -i 's#^  "crates/docket-core",#  "crates/docket-core",\n  "crates/seeded",#' "$tmp/Cargo.toml"
  out=$(layers_check "$tmp") && { echo "self-test: a crate missing from LAYERS was not caught"; rc=1; }
  grep -q '^LAYER: seeded missing-layer' <<<"$out" || { echo "self-test: no missing-layer line for seeded"; rc=1; }
  cp "$tmp/seed.bak" "$tmp/Cargo.toml"
  [ "$rc" -eq 0 ] && echo "self-test: the clean copy passes and all five seeded violations fail"
  return "$rc"
}

if [ "${1:-}" = "--self-test" ]; then
  self_test
  exit $?
fi
cd "$(dirname "$0")/.."

# RULES: what a crate reaches through ANY path (transitive, default features).
#
# The pure crates (docket-core, policy-point, action-review, docket-router, companion-wire,
# agent-loop, voice-wire, voice-loop) never reach a bus, a runtime, an HTTP client, an audio or
# display or accessibility stack, a database, a file watcher, an embedding runtime or the MCP SDK.
# Only policy-point reaches cedar-policy, and the crates above it (docket-router, the fakes and
# the daemons that run the router; docket-client only behind its `in_process` feature). Only actions-mcp reaches rmcp. docket-core
# and the pure crates beside it never reach toml: manifest TOML is parsed in docket-router.
EFFECTS="zbus zvariant tokio reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
NO_CEDAR="zbus zvariant tokio reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp"
RULES=(
  "docket-core: $EFFECTS toml"
  "policy-point: $NO_CEDAR toml"
  "action-review: $EFFECTS toml"
  "docket-router: $NO_CEDAR"
  # Skills are files read from directories handed in: no bus, runtime, network or Cedar.
  "docket-skills: $EFFECTS"
  # The tools an outside agent is offered, as pure data: shared by the MCP edge and the ACP host.
  "actions-tools: $EFFECTS toml"
  "companion-wire: $EFFECTS toml"
  # Installs agents from the registry: reads files and runs curl and npm as processes, so no bus,
  # runtime, network library or Cedar. It reads its own records as TOML.
  "docket-agents: $EFFECTS"
  "agent-loop: $EFFECTS toml"
  # The planner is portable: it asks any porter-client Transport (porter-client without its
  # `dbus` and `socket` features reaches no runtime), so inferd over D-Bus is companiond's
  # choice of transport, never the planner's.
  "docket-planner: $EFFECTS"
  # The in-app agent: the router (and with it Cedar) hosted in one app, no bus, no runtime, no
  # HTTP, no watcher. docket-client only behind `in_process`, never `dbus`.
  # The task model is portable: the companion's tasks over any router link and model transport,
  # a clock and a surface that are passed in; no bus, no runtime (the daemon's surface is the
  # one that holds tokio).
  "docket-tasks: $EFFECTS"
  # The durable session is portable and pure: no bus, runtime, HTTP, Cedar or toml; the log is a seam.
  "docket-session: $EFFECTS toml"
  # Restore points: the rules and the store seam, pure; the git store is another crate (a process).
  "docket-checkpoint: $EFFECTS toml"
  # The git store runs git as a process through tokio: no bus, HTTP, desktop stack, database,
  # Cedar, MCP SDK or toml.
  "docket-checkpoint-git: zbus zvariant reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy toml"
  # The ACP server edge: the lib is portable (the runtime is only the `server` feature's binary);
  # no bus, HTTP, Cedar or MCP SDK. The protocol's types come from agent-client-protocol-schema.
  # The client edge reaches the router only through docket-client (its `InProcess` transport is a
  # dev-dependency): the host of an external agent calls the same Intents1 every caller does.
  "docket-acp: zbus zvariant reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # Starts the external agents: the bus is the `dbus` feature's, the runtime is its processes'; no
  # HTTP, desktop stack, database, Cedar or MCP SDK, and nothing that links a sandbox library.
  "docket-launch: reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy landlock libseccomp"
  "docket-inapp: $NO_CEDAR"
  # The docket-acp process (and `docket-agent`, the host of an external coding agent): the router
  # and the companion hosted in it, over the bus (zbus, tokio) and nothing else of the desktop.
  "docket-acp-bin: reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp"
  # The models, the quarantined reader and the memory seam are portable too (moved out of
  # intentd and readerd): they ask any porter-client or almanac-client Transport, never a bus or
  # a runtime; almanac-client hosts its service only behind `in_process` (a dev-dependency here)
# and reaches the bus only behind `dbus`. docket-memory sits on the router, so Cedar is below it.
  "docket-models: $EFFECTS"
  "docket-reader: $EFFECTS"
  "docket-memory: $NO_CEDAR"
  "voice-wire: $EFFECTS toml"
  "voice-loop: $EFFECTS toml"
  # The caller's side of Voice1: zbus (with its tokio feature) and nothing of the microphone or speech engines.
  "voice-dbus: reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp cedar-policy"
  # zbus lives in docket-dbus, and in docket-client only behind its `dbus` feature.
  "docket-dbus: reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp cedar-policy"
  # A client never links the router: `InProcess` (feature `in_process`) is the one way in.
  "docket-client: $EFFECTS toml"
  "docket-fake: $NO_CEDAR"
  "docket-eval: $NO_CEDAR"
  # Test only: a private bus for the daemons' tests; nothing of the router, Cedar or the effects.
  "docket-testbus: docket-router policy-point action-review cedar-policy reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp"
  # quire-do is a thin client: it reaches the bus only through docket-client (LAYERS
  # keeps it a C or A crate that cannot reach the router), never the router, the policy point, the reviewer's
  # cascade or Cedar: a terminal has no way around intentd's gate.
  "companion-client: docket-router policy-point action-review cedar-policy reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp"
  "docket-cli: docket-router policy-point action-review cedar-policy reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp"
  "docket-ds: zbus zvariant tokio reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite notify rmcp"
  # The edge reaches rmcp, tokio and, as a bus client of intentd (docket-client `dbus`), zbus; nothing else of the effects.
  "actions-mcp: reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite notify"
  # The settings reader is pure but for reading the file: no bus, no runtime, no watcher (intentd
  # owns the directory watch), so actions-mcp, which may not link one, can share the reader.
  "docket-settings: $EFFECTS"
  # The daemons reach the bus and a runtime; none reaches HTTP, an embedding runtime or the MCP
  # SDK. almanac-client hosts the service only behind its `in_process` feature, so none reaches SQLite.
  "intentd: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  "companiond: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  "readerd: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  # Test only: the acceptance run links the docket daemons to run them as processes (memoryd and inferd are built by build.rs, not linked); no HTTP,
  # audio, embedding runtime or MCP SDK.
  # (The acceptance run starts the actions-mcp bridge as a process for the ACP engine, so it links rmcp through it.)
  "docket-accept: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite"
  "docket-log: $EFFECTS toml"
  "voiced: reqwest hyper hyper-util rustls ort fastembed rusqlite rmcp cedar-policy"
)
fail=0

for rule in "${RULES[@]}"; do
  crate="${rule%%:*}"
  read -r -a forbidden <<<"${rule#*:}"
  # A crate that cargo cannot find would make every check below pass vacuously.
  if ! cargo tree -p "$crate" --depth 0 >/dev/null 2>&1; then
    echo "ERROR: cargo tree cannot resolve $crate; the boundary was not checked"
    fail=1
    continue
  fi
  leaked=0
  # docket-client's default feature is the desktop transport (`dbus`); the rule is about the
  # portable build, so it is read without it. The `dbus` feature is a C-tier crate's.
  nodef=()
  [ "$crate" = docket-client ] && nodef=(--no-default-features)
  for dep in "${forbidden[@]}"; do
    if [ -n "$(cargo tree -p "$crate" "${nodef[@]}" -i "$dep" -e normal,build 2>/dev/null)" ]; then
      echo "LEAK: $crate depends on $dep"
      cargo tree -p "$crate" "${nodef[@]}" -i "$dep" -e normal,build 2>/dev/null | head -20
      leaked=1
      fail=1
    fi
  done
  if [ "$leaked" -eq 0 ]; then
    echo "boundary holds: $crate reaches none of ${forbidden[*]}"
  fi
done

layers_check . || fail=1

# docket sits below cua and sill in the repo order (stoker, porter, almanac, docket, cua, sill):
# nothing of theirs may enter any tree of this workspace, dev-dependencies included.
THEIRS="cuad cua-bus cua-run cua-desktop cua-dbus cua-fake a11y-tree seat-input maskcap quire-agent-protocol"
workspace_tree=$(cargo tree --workspace -e normal,build,dev --prefix none --all-features 2>/dev/null | awk '{print $1}' | sort -u)
theirs_found=0
for name in $THEIRS; do
  if grep -qx "$name" <<<"$workspace_tree"; then
    echo "LEAK: the workspace depends on $name (cua), which sits above docket"
    theirs_found=1
    fail=1
  fi
done
if grep -q '^sill-' <<<"$workspace_tree"; then
  echo "LEAK: the workspace depends on a sill crate, which sits above docket"
  theirs_found=1
  fail=1
fi
if [ "$theirs_found" -eq 0 ]; then
  echo "boundary holds: no cua or sill crate enters the workspace"
fi

# No shipped crate carries a test-only switch. The proc root of the callers and the model tap are
# arguments (`HarnessSeams`) that only the docket-accept mains pass; no daemon source reads
# `*_PROC_ROOT` or `DOCKET_MODEL_TRACE`. porter reads a fixture tree only with porter-dbus's
# `test-proc-root`, which docket-dbus's `fixture-callers` forwards (that one line is the only
# mention allowed); docket-accept and tests' dev-dependencies turn it on.
leaks=$(grep -rnE 'PROC_ROOT|DOCKET_MODEL_TRACE|test-proc-root|test-model-trace' crates \
  --include='*.rs' --include=Cargo.toml --exclude-dir=docket-accept --exclude-dir=tests \
  | grep -vE '^crates/docket-dbus/Cargo.toml:[0-9]+:fixture-callers = \["porter-dbus/test-proc-root"\]$' || true)
if [ -n "$leaks" ]; then
  echo "TEST SWITCH: a shipped crate names a harness switch:"
  echo "$leaks"
  fail=1
else
  echo "no shipped crate carries a proc-root or model-trace switch"
fi

# Built as the installer builds it (`-p` each shipped package, no dev-dependencies), no shipped
# package turns on the fixture tree. (A whole-workspace build unifies docket-accept's features, so
# the installer never builds one.)
for pkg in intentd companiond readerd actions-mcp docket-cli docket-acp-bin voiced; do
  # `-e features` elides the edge a package's own feature makes; the feature list per package
  # (`{p} {f}`) shows it.
  # A tree cargo could not print would read as "off": that is an error, not a pass.
  if ! tree=$(cargo tree -p "$pkg" -e normal,build --prefix none --format '{p} {f}' 2>&1); then
    echo "ERROR: cargo tree failed for $pkg; the fixture tree was not checked"
    echo "$tree" | tail -5
    fail=1
  elif grep -qE 'fixture-callers|test-proc-root' <<<"$tree"; then
    echo "TEST SWITCH: $pkg's build turns on the fixture tree"
    fail=1
  else
    echo "fixture tree off in $pkg's build"
  fi
done

# Every workspace member has a RULES row, so a new crate cannot slip in unchecked (LAYERS rows are
# checked above). The rows are matched from a here-string, not a pipe: under pipefail, `grep -q`
# leaving early would kill printf and fail the check at random.
rule_rows=$(printf '%s\n' "${RULES[@]}")
for member in $(sed -n 's#^  "crates/\(.*\)",$#\1#p' Cargo.toml); do
  grep -q "^$member:" <<<"$rule_rows" || { echo "ERROR: $member has no row in RULES"; fail=1; }
done

exit "$fail"
