#!/usr/bin/env bash
# One-time fill of the cloud account home used by `--accountd-home` (docs/live-eval.md, "Cloud").
#
#   dev/live/cloud-key.sh DIR ACCOUNTD [--key-file FILE] [--providers DIR]
#
# DIR       absolute, outside every git work tree; made 0700 here. Holds accountd.keys (0600), which
#           only accountd opens, and accountd's non-secret records (state/, data/).
# ACCOUNTD  porter's accountd built with `--features test-proc-root,test-keys`.
# --key-file FILE   a 0600 file holding the OpenRouter key on one line, outside every git work
#           tree. It is piped to accountd's standard input (never put in argv or the environment,
#           never printed or read by this script). Without it, accountd asks at its own prompt
#           with echo off.
# --providers DIR   where openrouter.toml is (default $PORTER_PROVIDERS, else ~/porter/providers).
#
# accountd runs on a private bus (dbus-run-session): the real session bus, the real Secret Service
# and the real home are never touched.
set -euo pipefail

die() { echo "cloud-key: $*" >&2; exit 2; }
loose() { [ -n "$(find "$1" -maxdepth 0 -perm /077)" ]; }
in_git() { (cd "$(dirname "$1")" && git rev-parse --is-inside-work-tree >/dev/null 2>&1); }

[ $# -ge 2 ] || die "usage: cloud-key.sh DIR ACCOUNTD [--key-file FILE] [--providers DIR]"
home=$1; accountd=$2; shift 2
keyfile=; providers=${PORTER_PROVIDERS:-$HOME/porter/providers}
while [ $# -gt 0 ]; do
  case "$1" in
    --key-file) [ $# -ge 2 ] || die "--key-file needs a value"; keyfile=$2; shift 2 ;;
    --providers) [ $# -ge 2 ] || die "--providers needs a value"; providers=$2; shift 2 ;;
    *) die "unknown option $1" ;;
  esac
done

case "$home" in /*) ;; *) die "DIR must be absolute" ;; esac
[ -x "$accountd" ] || die "$accountd is not an executable"
[ -f "$providers/openrouter.toml" ] || die "no $providers/openrouter.toml (pass --providers)"
mkdir -p "$home"
home=$(cd "$home" && pwd -P)
in_git "$home/x" && die "$home is inside a git work tree; keys must live outside every repository"
chmod 700 "$home"
if [ -n "$keyfile" ]; then
  [ -f "$keyfile" ] || die "$keyfile is not a file"
  keyfile=$(cd "$(dirname "$keyfile")" && pwd -P)/$(basename "$keyfile")
  in_git "$keyfile" && die "$keyfile is inside a git work tree"
  ! loose "$keyfile" || die "$keyfile has group or other permission bits; chmod 600 it"
fi

mkdir -p "$home/state" "$home/config" "$home/data/porter/providers" "$home/run"
chmod 700 "$home/run"
cp "$providers/openrouter.toml" "$home/data/porter/providers/"
printf '[[caller]]\napp = "org.quire.Inference"\nrole = "porter_daemon"\n' > "$home/config/porter-callers.toml"

add=(add openrouter --allow org.quire.Intents --allow org.quire.Companion --allow org.quire.Reader)
run() {
  env -i PATH="$PATH" HOME="$home" XDG_STATE_HOME="$home/state" XDG_CONFIG_HOME="$home/config" \
    XDG_DATA_HOME="$home/data" XDG_RUNTIME_DIR="$home/run" XDG_CACHE_HOME="$home/run" \
    ACCOUNTD_KEYS="file:$home/accountd.keys" \
    dbus-run-session -- "$accountd" "${add[@]}"
}
if [ -n "$keyfile" ]; then
  # The key line, then the answer to "Add this account?" (a blank line is the default, yes).
  { cat -- "$keyfile"; printf '\ny\n'; } | run
else
  run
fi
chmod 600 "$home/accountd.keys"
echo "cloud-key: done. Use: --accountd $accountd --accountd-home $home" >&2
