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
  # Skills are files read from directories handed in: no bus, runtime, network or Cedar.
  "docket-skills: $EFFECTS"
  "companion-wire: $EFFECTS toml"
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
  # The ACP server edge: the lib is portable (the runtime is only the `server` feature's binary);
  # no bus, HTTP, Cedar or MCP SDK. The protocol's types come from agent-client-protocol-schema.
  "docket-acp: zbus zvariant reqwest hyper hyper-util rustls pipewire wayland-client wayland-backend reis atspi oo7 ort fastembed rusqlite notify rmcp cedar-policy"
  # The sandboxed shell tool: bubblewrap is a separate process (std only), so no runtime, bus,
  # HTTP, Cedar or toml; and nothing that links a sandbox library.
  "docket-shell: $EFFECTS toml landlock libseccomp"
  "docket-inapp: $NO_CEDAR"
  # The docket-acp process: the router and the companion hosted in it, over the bus (zbus, tokio) and
  # nothing else of the desktop.
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
  # docket-client's default feature is the desktop transport (`dbus`); the rule is about the
  # portable build, so it is read without it. The `dbus` edge is held by the EDGES row below.
  nodef=()
  [ "$crate" = docket-client ] && nodef=(--no-default-features)
  for dep in "${forbidden[@]}"; do
    if cargo tree -p "$crate" "${nodef[@]}" -i "$dep" -e normal,build 2>/dev/null | grep -q .; then
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
  "docket-skills: docket-core porter-core prov"
  "policy-point: docket-core porter-core prov"
  "action-review: docket-core model-provider porter-core porter-infer prov"
  "docket-router: action-review almanac-core docket-core docket-session docket-skills policy-point porter-core prov"
  "companion-wire: almanac-core docket-core porter-core porter-infer prov"
  "agent-loop: almanac-core companion-wire docket-core porter-core prov"
  "docket-planner: agent-loop almanac-core companion-wire docket-core porter-client porter-core porter-infer prov"
  "docket-tasks: agent-loop almanac-core companion-wire docket-client docket-core docket-planner docket-session docket-skills porter-client porter-core porter-infer prov"
  "docket-session: companion-wire docket-core porter-core prov"
  "docket-acp: companion-wire docket-core docket-session docket-settings docket-shell porter-core prov"
  "docket-shell: docket-core"
  "docket-acp-bin: docket-acp docket-client docket-core docket-dbus docket-planner docket-router docket-tasks porter-core prov"
  "docket-models: action-review docket-core porter-client porter-core porter-infer prov"
  "docket-reader: docket-core docket-models porter-client porter-core porter-infer prov"
  "docket-memory: almanac-client almanac-core docket-core docket-router docket-session porter-core prov"
  "docket-inapp: action-review agent-loop almanac-core companion-wire docket-client docket-core docket-memory docket-models docket-planner docket-reader docket-router docket-session docket-skills docket-tasks policy-point porter-client porter-core prov"
  "docket-dbus: docket-core porter-client porter-core porter-dbus porter-infer prov"
  "docket-client: docket-core docket-dbus docket-router prov"
  "docket-fake: action-review almanac-core docket-client docket-core docket-router docket-session policy-point porter-core prov"
  "docket-eval: docket-core docket-fake docket-skills docket-router porter-core prov"
  "docket-testbus: docket-dbus"
  "actions-mcp: docket-client docket-core docket-dbus docket-settings porter-core prov"
  "intentd: action-review almanac-client almanac-core docket-client docket-core docket-dbus docket-memory docket-models docket-router docket-session docket-settings docket-skills policy-point porter-client porter-core porter-dbus porter-infer prov"
  "companiond: companion-wire docket-client docket-core docket-dbus docket-planner docket-settings docket-skills docket-tasks porter-client porter-core prov"
  "readerd: docket-client docket-core docket-dbus docket-reader porter-client porter-core porter-infer prov"
  "docket-accept: action-review almanac-client almanac-core companion-wire companiond docket-acp docket-acp-bin docket-cli docket-client docket-core docket-dbus docket-eval docket-fake docket-router docket-testbus intentd porter-client porter-core porter-infer prov readerd"
  "companion-client: companion-wire docket-client docket-core docket-dbus prov"
  "docket-cli: companion-client companion-wire docket-client docket-core docket-skills model-provider porter-core prov"
  "docket-settings: docket-core porter-core"
  "docket-ds: companion-wire docket-client docket-core ds-intents porter-core prov voice-wire"
  "voice-wire: docket-core porter-core porter-infer"
  "voice-loop: docket-core porter-core porter-infer voice-wire"
  "voiced: docket-core porter-client porter-core porter-dbus porter-infer speech-provider speech-vad voice-loop voice-wire"
)
for edge in "${EDGES[@]}"; do
  crate="${edge%%:*}"
  read -r -a allowed <<<"${edge#*:}"
  found=$(cargo tree -p "$crate" --depth 1 -e normal,build --prefix none --all-features 2>/dev/null \
    | grep -E '\((/|https://github.com/PoHsuanLai/)' | awk '{print $1}' | grep -vx "$crate" | sort -u | tr '\n' ' ')
  want=$(printf '%s\n' "${allowed[@]}" | grep . | sort -u | tr '\n' ' ')
  if [ "$found" != "$want" ]; then
    echo "EDGE: $crate depends on [${found% }], the table allows [${want% }]"
    fail=1
  else
    echo "edges hold: $crate depends on [${found% }]"
  fi
done

# The test-only feature is never on in a default build (a dist build uses default features):
# `test-proc-root` (INTENTD_PROC_ROOT, COMPANIOND_PROC_ROOT), the only way to make a daemon read
# callers from somewhere other than /proc. Neither the daemon's own default features nor any
# dependent's enable it.
for daemon in intentd companiond; do
  enabled=$(cargo tree -p "$daemon" -e normal,build -f '{p} [{f}]' --prefix none 2>/dev/null \
    | grep -E "^$daemon " | grep -oE '\[[^]]*\]' | tr ',[]' '\n\n\n')
  if printf '%s\n' "$enabled" | grep -qE '^test-'; then
    echo "TEST FEATURE: a default build of $daemon enables: $(printf '%s\n' "$enabled" | grep -E '^test-' | tr '\n' ' ')"
    fail=1
  else
    echo "test features (test-proc-root) are off in a default build of $daemon"
  fi
done

# Every workspace member has a row above, so a new crate cannot slip in unchecked.
for member in $(sed -n 's#^  "crates/\(.*\)",$#\1#p' Cargo.toml); do
  printf '%s\n' "${EDGES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in EDGES"; fail=1; }
  printf '%s\n' "${RULES[@]}" | grep -q "^$member:" || { echo "ERROR: $member has no row in RULES"; fail=1; }
done

exit "$fail"
