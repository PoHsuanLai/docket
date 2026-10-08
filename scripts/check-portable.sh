#!/usr/bin/env bash
# The portable core builds without the desktop (quire design/36 section 4): exactly the crates
# below, with no default features, must `cargo check`, and nothing in their trees may be a
# Linux-only or desktop-bus crate. Needs no network (cargo reads the lockfile and the cache).
#
# The portable core is the in-app agent: the vocabulary, the router and its Cedar policy point,
# the reviewer, the agent loop, the planner over any porter-client Transport, skills, the red
# team's suite, the fakes, the model-backed reviewer and policy writer (`docket-models`), the
# quarantined reader (`docket-reader`), the memory seam (`docket-memory`), and `docket-inapp`,
# which hosts them in one app. The desktop extras
# (docket-dbus, intentd, companiond, readerd, voiced, quire-do, actions-mcp, the acceptance
# crate and everything that names a bus) are not on this list on purpose.
#
# Cross targets are checked when rustup already has them installed; this script never installs
# one and says which are missing.
set -euo pipefail
cd "$(dirname "$0")/.."

PORTABLE=(
  docket-core docket-skills policy-point action-review docket-router
  companion-wire agent-loop docket-planner docket-tasks docket-session docket-shell docket-acp docket-client docket-fake docket-eval
  docket-models docket-reader docket-memory docket-inapp voice-wire voice-loop
)
# What must never be in a portable crate's tree: a bus, or a Linux-only kernel interface.
FORBIDDEN='zbus|zvariant|inotify|landlock|pipewire|notify|libspa'
# The targets the owner named, then any other installed Windows or macOS target worth a check.
TARGETS=(x86_64-apple-darwin x86_64-pc-windows-gnu x86_64-pc-windows-msvc aarch64-apple-darwin)

packages=()
for crate in "${PORTABLE[@]}"; do packages+=(-p "$crate"); done

fail=0
echo "== cargo check --no-default-features on ${#PORTABLE[@]} portable crates"
cargo check "${packages[@]}" --no-default-features --all-targets || fail=1

echo "== the portable crates' trees hold no bus and no Linux-only crate"
for crate in "${PORTABLE[@]}"; do
  tree=$(cargo tree -p "$crate" --no-default-features -e normal,build --prefix none 2>&1) || {
    echo "ERROR: cargo tree cannot resolve $crate"; fail=1; continue; }
  hits=$(printf '%s\n' "$tree" | awk '{print $1}' | grep -E "^($FORBIDDEN)$" | sort -u || true)
  if [ -n "$hits" ]; then
    echo "LEAK: $crate reaches: $(printf '%s' "$hits" | tr '\n' ' ')"
    fail=1
  else
    echo "portable: $crate reaches none of ${FORBIDDEN//|/ }"
  fi
done

echo "== cross targets"
# `rustup` answers for the toolchain this repo pins (rust-toolchain.toml).
installed=$(rustup target list --installed 2>/dev/null || true)
for target in "${TARGETS[@]}"; do
  if ! printf '%s\n' "$installed" | grep -qx "$target"; then
    echo "SKIP $target: not installed (rustup target add $target)"
    continue
  fi
  echo "-- cargo check --target $target"
  # cedar-policy reaches `psm`, which assembles a file for the target: clang does that for any
  # target when it is on PATH. A target whose archiver or assembler is missing ends in a cc-rs
  # error, which says "not verified here", never "not portable": any other error fails.
  tag=${target//-/_}
  clang_cc=()
  if command -v clang >/dev/null 2>&1; then clang_cc=("CC_$tag=clang"); fi
  if out=$(env "${clang_cc[@]}" cargo check "${packages[@]}" --no-default-features --target "$target" 2>&1); then
    echo "ok $target"
  elif printf '%s\n' "$out" | grep -q 'error occurred in cc-rs'; then
    echo "UNVERIFIED $target: a C cross toolchain is missing (cc-rs failed in a dependency's build script)"
  else
    printf '%s\n' "$out" | tail -40
    fail=1
  fi
done

if [ "$fail" -eq 0 ]; then echo "PORTABLE GREEN"; fi
exit "$fail"
