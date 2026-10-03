#!/usr/bin/env bash
# Try `quire-do` by hand against a real intentd, on a private bus that is not your session's:
# a scratch dbus-daemon, a scratch HOME and XDG directories, intentd, a fake mail app
# (org.quire.Mail) and a fake sill sheet that asks on THIS terminal. Nothing of your real
# session bus or ~/.config is named.
#
#   scripts/try-quire-do.sh          # starts it all; prints what to run in a second terminal
#
# In the second terminal (the script prints the exact `source` line):
#   quire-do apps
#   quire-do mail --list
#   quire-do mail thread.read t2                 # a read: runs, no sheet
#   quire-do mail thread.archive t2 --dry-run    # the preview, no sheet
#   quire-do mail thread.archive t2              # a write: the sheet appears HERE; answer y, t or n
#   quire-do undo --last
#   quire-do memory forget memory.fact:f1        # hidden: exit 3, no sheet
# Ctrl-C here stops everything.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build -p intentd -p docket-cli --bins --examples
bin="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')/debug"

world="$(mktemp -d)"
pids=()
cleanup() {
  for pid in "${pids[@]}"; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$world"
}
trap cleanup EXIT

mkdir -p "$world/data/quire/intents" "$world/config" "$world/bin"
cp crates/docket-fake/fixtures/manifests/org.quire.Mail.intents.toml "$world/data/quire/intents/org.quire.Mail.toml"
cp manifests/org.quire.Memory.toml "$world/data/quire/intents/org.quire.Memory.toml"
cat > "$world/bus.conf" <<CONF
<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:path=$world/bus.sock</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow send_destination="*"/>
    <allow receive_sender="*"/>
    <allow own="*"/>
  </policy>
</busconfig>
CONF

dbus-daemon --config-file="$world/bus.conf" --nofork --print-address=1 > "$world/address" &
pids+=($!)
for _ in $(seq 50); do [ -s "$world/address" ] && break; sleep 0.1; done
bus="$(head -n1 "$world/address")"

# What every process of this little world sees; nothing else is passed on.
scratch=(env -i HOME="$world" PATH="$PATH" XDG_RUNTIME_DIR="$world" XDG_DATA_HOME="$world/data"
  XDG_DATA_DIRS="$world/none" XDG_CONFIG_HOME="$world/config" XDG_CONFIG_DIRS="$world/none"
  DBUS_SESSION_BUS_ADDRESS="$bus" DBUS_SYSTEM_BUS_ADDRESS="$bus")

"${scratch[@]}" "$bin/intentd" &
pids+=($!)
ln -s "$bin/quire-do" "$world/bin/quire-do"
cat > "$world/env" <<ENV
export DBUS_SESSION_BUS_ADDRESS='$bus'
export PATH='$world/bin':\$PATH
ENV

echo "In a second terminal:  source $world/env"
echo
"${scratch[@]}" "$bin/examples/try_apps"
