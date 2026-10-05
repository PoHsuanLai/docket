#!/usr/bin/env bash
# Crate boundaries, mechanically enforced (ARCHITECTURE.md section 1 is the table; this is its
# mechanical form).
#
# `cargo tree -i <dep>` exits 101 when the dependency is absent, which is precisely the state
# we want. Checking the exit status would therefore fail whenever the boundary holds, so we
# check for OUTPUT instead: any line naming the dependency is a leak.
set -uo pipefail
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
  "companion-wire: $EFFECTS toml"
  "agent-loop: $EFFECTS toml"
  "voice-wire: $EFFECTS toml"
  "voice-loop: $EFFECTS toml"
  # zbus lives in docket-dbus, and in docket-client only behind its `dbus` feature.
  "docket-dbus: reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp cedar-policy"
  # A client never links the router: `InProcess` (feature `in_process`) is the one way in.
  "docket-client: $EFFECTS toml"
  "docket-fake: $NO_CEDAR"
  "docket-eval: $NO_CEDAR"
  # Test only: a private bus for the daemons' tests; nothing of the router, Cedar or the effects.
  "docket-testbus: docket-router policy-point action-review cedar-policy reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp"
  # quire-do is a thin client: it reaches the bus only through docket-client (the EDGES row below
  # forbids a direct docket-dbus or zbus), never the router, the policy point, the reviewer's
  # cascade or Cedar: a terminal has no way around intentd's gate.
  "docket-cli: docket-router policy-point action-review cedar-policy reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite rmcp"
  "docket-ds: zbus zvariant tokio reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite notify rmcp"
  # The edge reaches rmcp, tokio and, as a bus client of intentd (docket-client `dbus`), zbus; nothing else of the effects.
  "actions-mcp: reqwest hyper hyper-util rustls pipewire oo7 ort fastembed rusqlite notify"
  # The daemons reach the bus and a runtime; none reaches HTTP, an embedding runtime or the MCP
  # SDK. almanac-client hosts the service only behind its `in_process` feature, so none reaches SQLite.
  "intentd: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  "companiond: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  "readerd: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
  # Test only: the acceptance run links the docket daemons to run them as processes (memoryd and inferd are built by build.rs, not linked); no HTTP,
  # audio, embedding runtime or MCP SDK.
  "docket-accept: reqwest hyper hyper-util rustls pipewire ort fastembed rusqlite rmcp"
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
  for dep in "${forbidden[@]}"; do
    if cargo tree -p "$crate" -i "$dep" -e normal,build 2>/dev/null | grep -q .; then
      echo "LEAK: $crate depends on $dep"
      cargo tree -p "$crate" -i "$dep" -e normal,build 2>/dev/null | head -20
      leaked=1
      fail=1
    fi
  done
  if [ "$leaked" -eq 0 ]; then
    echo "boundary holds: $crate reaches none of ${forbidden[*]}"
  fi
done

# docket sits below cua and sill in the repo order (stoker, porter, almanac, docket, cua, sill):
# nothing of theirs may enter any tree of this workspace, dev-dependencies included.
THEIRS="cuad cua-bus cua-run cua-desktop cua-dbus cua-fake a11y-tree seat-input maskcap quire-agent-protocol"
workspace_tree=$(cargo tree --workspace -e normal,build,dev --prefix none --all-features 2>/dev/null | awk '{print $1}' | sort -u)
theirs_found=0
for name in $THEIRS; do
  if printf '%s\n' "$workspace_tree" | grep -qx "$name"; then
    echo "LEAK: the workspace depends on $name (cua), which sits above docket"
    theirs_found=1
    fail=1
  fi
done
if printf '%s\n' "$workspace_tree" | grep -q '^sill-'; then
  echo "LEAK: the workspace depends on a sill crate, which sits above docket"
  theirs_found=1
  fail=1
fi
if [ "$theirs_found" -eq 0 ]; then
  echo "boundary holds: no cua or sill crate enters the workspace"
fi

# The allowed edges between our own crates: each crate's DIRECT normal and build path
# dependencies (all features), and nothing else. A dependency not listed is a leak; so is one
# the crate no longer has, so the table stays exact. Dev dependencies are outside it.
EDGES=(
  "docket-core: almanac-core cua-action model-provider porter-core prov"
  "policy-point: docket-core porter-core prov"
  "action-review: docket-core model-provider porter-core porter-infer prov"
  "docket-router: action-review almanac-core docket-core policy-point porter-core prov"
  "companion-wire: almanac-core docket-core porter-core porter-infer prov"
  "agent-loop: almanac-core companion-wire docket-core porter-core prov"
  "docket-dbus: docket-core porter-client porter-dbus prov"
  "docket-client: docket-core docket-dbus docket-router prov"
  "docket-fake: action-review almanac-core docket-client docket-core docket-router policy-point porter-core prov"
  "docket-eval: docket-core docket-fake docket-router porter-core prov"
  "docket-testbus: docket-dbus"
  "actions-mcp: docket-client docket-core docket-dbus porter-core prov"
  "intentd: action-review almanac-client almanac-core docket-client docket-core docket-dbus docket-router policy-point porter-client porter-core porter-infer prov"
  "companiond: agent-loop almanac-core companion-wire docket-client docket-core docket-dbus porter-client porter-core porter-infer prov"
  "readerd: docket-client docket-core docket-dbus porter-client porter-core porter-infer prov"
  "docket-accept: almanac-client almanac-core companion-wire companiond docket-client docket-core docket-dbus docket-router docket-testbus intentd porter-core porter-infer prov readerd"
  "docket-cli: docket-client docket-core model-provider porter-core prov"
  "docket-ds: companion-wire docket-client docket-core ds-intents porter-core prov voice-wire"
  "voice-wire: docket-core porter-core porter-infer"
  "voice-loop: docket-core porter-core porter-infer voice-wire"
  "voiced: docket-core porter-client porter-core porter-infer speech-vad voice-loop voice-wire"
)
for edge in "${EDGES[@]}"; do
  crate="${edge%%:*}"
  read -r -a allowed <<<"${edge#*:}"
  found=$(cargo tree -p "$crate" --depth 1 -e normal,build --prefix none --all-features 2>/dev/null \
    | grep '(/' | awk '{print $1}' | grep -vx "$crate" | sort -u | tr '\n' ' ')
  want=$(printf '%s\n' "${allowed[@]}" | grep . | sort -u | tr '\n' ' ')
  if [ "$found" != "$want" ]; then
    echo "EDGE: $crate depends on [${found% }], the table allows [${want% }]"
    fail=1
  else
    echo "edges hold: $crate depends on [${found% }]"
  fi
done

# Every workspace member has a row above, so a new crate cannot slip in unchecked.
for member in $(sed -n 's#^  "crates/\(.*\)",$#\1#p' Cargo.toml); do
  printf '%s\n' "${EDGES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in EDGES"; fail=1; }
  printf '%s\n' "${RULES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in RULES"; fail=1; }
done

exit "$fail"
