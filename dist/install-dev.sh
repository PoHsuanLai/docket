#!/usr/bin/env bash
# install-dev.sh: install the agent stack for ONE user, from sibling checkouts, with no root.
#
#   dist/install-dev.sh                       build (release, --locked) and install under ~/.local
#   dist/install-dev.sh --prefix DIR          binaries in DIR/bin (default: $PREFIX, else ~/.local)
#   dist/install-dev.sh --cloud               also let inferd reach the network (hosted models); without it inferd is offline
#   dist/install-dev.sh --dry-run             print every action; write nothing, build nothing
#   dist/install-dev.sh --uninstall           remove exactly what the last install put down
#
# The checkouts are the siblings of this one: ../porter (inferd, accountd), ../almanac (memoryd),
# ../stoker (the model catalog inferd reads) and this checkout (intentd, companiond, readerd,
# actions-mcp, quire-do, docket-eval). voiced (a skeleton), syncd and cuad are not installed.
#
# What goes where (XDG_CONFIG_HOME and XDG_DATA_HOME are honoured):
#   $PREFIX/bin/                              the binaries
#   ~/.config/systemd/user/                   the units, ExecStart pointed at $PREFIX/bin; a drop-in
#                                             for inferd and accountd (see "dev-only changes")
#   ~/.local/share/dbus-1/services/           D-Bus activation files, Exec pointed at $PREFIX/bin
#   ~/.local/share/quire/{intents,skills,settings}/   manifests, skills, settings schemas
#   ~/.local/share/porter/providers/          provider files (openrouter, ...): the AI keys inferd holds and the identities accountd keeps
#   ~/.local/share/stoker/catalog/            the model catalogue, for inferd
#   ~/.config/quire/*.toml                    default configs, ONLY where absent (never overwritten)
#   ~/.local/state/quire-dev/install-dev.manifest   what was installed, for --uninstall
#
# The units' mount-namespace protections are drop-ins (<unit>.service.d/sandbox.conf), installed unless
# this system refuses them to user units (INSTALL_DEV_SANDBOX=auto, on or off). Dev-only change to
# them: ProtectHome=yes becomes read-only for companiond and readerd
# when the prefix is under $HOME (with `yes` the binary itself cannot be executed from $HOME, and
# companiond could not read its config and skills). The porter units need nothing: they make their own
# directories. --cloud installs porter's dist/inferd-cloud.conf as inferd.service.d/cloud.conf (network
# on); it is opt-in because local engines share inferd's sandbox, so an on-device-only setup keeps none.
# memoryd has no unit in almanac/dist: almanac/dbus/memoryd.service is its unit, started by D-Bus
# activation (org.quire.Memory1.service names it), and that is what is installed.
#
# Needs root, and is printed but never run: /etc/porter/callers.toml (the caller table accountd and
# inferd read, at startup only). After it, restart both: systemctl --user restart accountd inferd.
# AI keys are inferd's, not accountd's: add one with `inferd add <provider> --allow <app>` while
# inferd is stopped (accountd keeps identities only).
#
# INSTALL_DEV_BIN_DIR=DIR installs prebuilt binaries from DIR instead of building (the test does).
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DOCKET="$(cd "$HERE/.." && pwd)"
SIBLINGS="${INSTALL_DEV_SIBLINGS:-$(cd "$DOCKET/.." && pwd)}"
PORTER="$SIBLINGS/porter"
ALMANAC="$SIBLINGS/almanac"
STOKER="$SIBLINGS/stoker"
CUA="$SIBLINGS/cua"

PREFIX="${PREFIX:-$HOME/.local}"
DRY=0
UNINSTALL=0
CLOUD=0

usage() { sed -n '2,/^set -euo/p' "${BASH_SOURCE[0]}" | sed '$d' | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix) [ $# -ge 2 ] || { echo "--prefix needs a directory" >&2; exit 2; }; PREFIX="$2"; shift ;;
    --prefix=*) PREFIX="${1#--prefix=}" ;;
    --dry-run) DRY=1 ;;
    --uninstall) UNINSTALL=1 ;;
    --cloud) CLOUD=1 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done

[ "$(id -u)" -ne 0 ] || { echo "run this as yourself, not root" >&2; exit 1; }
case "$PREFIX" in /*) ;; *) echo "the prefix must be an absolute path: $PREFIX" >&2; exit 2 ;; esac

BIN="$PREFIX/bin"
CONFIG="${XDG_CONFIG_HOME:-$HOME/.config}"
DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
STATE="${XDG_STATE_HOME:-$HOME/.local/state}"
UNITS="$CONFIG/systemd/user"
DBUS="$DATA/dbus-1/services"
QUIRE_CONFIG="$CONFIG/quire"
MANIFEST="$STATE/quire-dev/install-dev.manifest"

# Binary -> the repo and package that builds it.
BINARIES=(
  "intentd docket intentd"
  "companiond docket companiond"
  "readerd docket readerd"
  "actions-mcp docket actions-mcp"
  "quire-do docket docket-cli"
  "docket-eval docket docket-eval"
  "memoryd almanac memoryd"
  "inferd porter inferd"
  "accountd porter accountd"
)
repo_dir() { case "$1" in docket) echo "$DOCKET" ;; almanac) echo "$ALMANAC" ;; porter) echo "$PORTER" ;; esac; }

# The installed set: "kind source dest" per line. Kinds: bin (copied, mode 755), unit and dbus
# (copied with the Exec rewritten), data (copied, mode 644), config (copied only if absent).
PLAN=()
plan() { PLAN+=("$1|$2|$3"); }

# The units' mount-namespace protections are a drop-in beside each unit (<unit>.d/sandbox.conf); they
# go in unless this system refuses them to user units (INSTALL_DEV_SANDBOX=on or off skips the test).
plan_sandbox() {
  [ "$SANDBOX_ON" = 1 ] || return 0
  [ -f "$1.d/sandbox.conf" ] || return 0
  plan sandbox "$1.d/sandbox.conf" "$UNITS/$(basename "$1").d/sandbox.conf"
}
plan_units() {
  local dir="$1" unit
  for unit in "$dir"/*.service; do
    [ -e "$unit" ] || continue
    case "$(basename "$unit")" in voiced.service|syncd.service|cuad.service) continue ;; esac
    plan unit "$unit" "$UNITS/$(basename "$unit")"
    plan_sandbox "$unit"
  done
}
plan_dbus() {
  local dir="$1" file
  for file in "$dir"/*.service; do
    [ -e "$file" ] || continue
    case "$(basename "$file")" in org.quire.Voice1.service|org.quire.Sync1.service) continue ;; esac
    plan dbus "$file" "$DBUS/$(basename "$file")"
  done
}
plan_tree() { # kind, source dir, dest dir: every regular file under it
  local kind="$1" src="$2" dest="$3" file
  [ -d "$src" ] || return 0
  while IFS= read -r file; do plan "$kind" "$src/$file" "$dest/$file"; done \
    < <(cd "$src" && find . -type f | sed 's|^\./||' | sort)
}

build_plan() {
  local entry name repo pkg
  for entry in "${BINARIES[@]}"; do
    read -r name repo pkg <<<"$entry"
    plan bin "$(bin_source "$name" "$repo")" "$BIN/$name"
  done
  plan_units "$DOCKET/dist"; plan_units "$PORTER/dist"
  plan unit "$ALMANAC/dbus/memoryd.service" "$UNITS/memoryd.service"
  plan_sandbox "$ALMANAC/dbus/memoryd.service"
  plan_dbus "$DOCKET/dist/dbus"; plan_dbus "$PORTER/dist/dbus"
  plan dbus "$ALMANAC/dbus/org.quire.Memory1.service" "$DBUS/org.quire.Memory1.service"
  plan_tree data "$DOCKET/dist/skills" "$DATA/quire/skills"
  local dir
  for dir in "$DOCKET" "$ALMANAC" "$PORTER" "$CUA"; do
    plan_tree data "$dir/dist/intents" "$DATA/quire/intents"
    plan_tree data "$dir/dist/settings" "$DATA/quire/settings"
  done
  plan data "$PORTER/dist/inferd.settings.toml" "$DATA/quire/settings/inferd.settings.toml"
  plan_tree data "$PORTER/providers" "$DATA/porter/providers"
  plan_tree data "$STOKER/catalog" "$DATA/stoker/catalog"
  local cfg
  for cfg in "$DOCKET"/dist/*.toml; do
    # The callers files are the system's (sudo_step prints them): a fixed role (sill's sheet_host)
    # comes from /etc/quire alone, and the one in the person's config is ignored.
    case "$(basename "$cfg")" in voiced.toml|*-callers.toml) continue ;; esac
    plan config "$cfg" "$QUIRE_CONFIG/$(basename "$cfg")"
  done
  if [ "$CLOUD" = 1 ]; then plan dropin "$PORTER/dist/inferd-cloud.conf" "$UNITS/inferd.service.d/cloud.conf"; fi
  plan config "$PORTER/dist/inferd.toml" "$QUIRE_CONFIG/inferd.toml"
  plan config "$ALMANAC/dbus/memory-callers.toml" "$QUIRE_CONFIG/memory-callers.toml"
}

bin_source() { # name repo -> the file to install
  if [ -n "${INSTALL_DEV_BIN_DIR:-}" ]; then echo "$INSTALL_DEV_BIN_DIR/$1"
  else echo "${CARGO_TARGET_DIR:-$(repo_dir "$2")/target}/release/$1"; fi
}

# ---- the manifest: path -> "kind sha", kept sorted ----
declare -A OWNED=()
load_manifest() {
  [ -f "$MANIFEST" ] || return 0
  local kind sha path
  while IFS=$'\t' read -r kind sha path; do
    [ -z "$kind" ] || OWNED["$path"]="$kind $sha"
  done <"$MANIFEST"
}
sha_of() { sha256sum "$1" | cut -d' ' -f1; }
write_manifest() {
  make_dir "$(dirname "$MANIFEST")"
  local path tmp="$MANIFEST.new"
  : >"$tmp"
  for path in "${!OWNED[@]}"; do
    read -r kind sha <<<"${OWNED[$path]}"
    printf '%s\t%s\t%s\n' "$kind" "$sha" "$path"
  done | sort -t$'\t' -k3 >>"$tmp"
  mv "$tmp" "$MANIFEST"
}

say() { printf '%s\n' "$*"; }
act() { say "  $*"; }

# ---- rewriting ----
under_home() { case "$BIN" in "$HOME"|"$HOME"/*) return 0 ;; *) return 1 ;; esac; }
rewrite_exec() { # the packaged /usr/libexec/quire/<name> is now $BIN/<name>
  sed -E -e "s|^(ExecStart=)/usr/libexec/quire/|\1$BIN/|" -e "s|^(Exec=)/usr/libexec/quire/|\1$BIN/|"
}
rewrite_unit() {
  local unit="$1"
  if under_home; then rewrite_exec | sed -E 's|^ProtectHome=yes$|ProtectHome=read-only|'; else rewrite_exec; fi <"$unit"
}
render() { # kind source -> the text to install, on stdout
  case "$1" in
    unit) rewrite_unit "$2" ;;
    sandbox) if under_home; then sed -E 's|^ProtectHome=yes$|ProtectHome=read-only|' "$2"; else cat "$2"; fi ;;
    dbus) rewrite_exec <"$2" ;;
    *) cat "$2" ;;
  esac
}

# make_dir DIR: mkdir -p, remembering the directories this made (they are removed again when empty).
make_dir() {
  local dir="$1" missing=() d
  d="$dir"
  while [ ! -d "$d" ] && [ "$d" != "/" ]; do missing=("$d" "${missing[@]}"); d="$(dirname "$d")"; done
  [ "${#missing[@]}" -gt 0 ] || return 0
  mkdir -p "$dir"
  for d in "${missing[@]}"; do OWNED["$d"]="dir -"; done
}

install_file() { # kind source dest
  local kind="$1" src="$2" dest="$3" tmp mode=644
  [ "$kind" != bin ] || mode=755
  if [ "$kind" = config ] && [ -e "$dest" ]; then act "kept (yours): $dest"; return 0; fi
  if [ "$DRY" = 1 ]; then act "install -m $mode $src -> $dest"; return 0; fi
  make_dir "$(dirname "$dest")"
  tmp="$(mktemp "$dest.XXXXXX")"
  if [ "$kind" = bin ]; then cat "$src" >"$tmp"; else render "$kind" "$src" >"$tmp"; fi
  chmod "$mode" "$tmp"
  mv "$tmp" "$dest"
  act "installed: $dest"
  OWNED["$dest"]="$kind $(sha_of "$dest")"
}

# An earlier install-dev put its own drop-ins here; the units no longer need them.
drop_stale() {
  local path
  for path in "$UNITS/inferd.service.d/10-dev.conf" "$UNITS/accountd.service.d/10-dev.conf"; do
    [ -n "${OWNED[$path]:-}" ] || continue
    if [ "$DRY" = 1 ]; then act "remove stale $path"; else rm -f "$path"; unset 'OWNED[$path]'; act "removed stale: $path"; fi
  done
}

build_all() {
  [ -z "${INSTALL_DEV_BIN_DIR:-}" ] || { say "using prebuilt binaries from $INSTALL_DEV_BIN_DIR"; return 0; }
  local repo
  for repo in docket almanac porter; do
    local packages=() entry name r pkg
    for entry in "${BINARIES[@]}"; do
      read -r name r pkg <<<"$entry"
      [ "$r" != "$repo" ] || packages+=(-p "$pkg")
    done
    if [ "$DRY" = 1 ]; then
      act "cargo build --release --locked --manifest-path $(repo_dir "$repo")/Cargo.toml ${packages[*]}"
    else
      cargo build --release --locked --manifest-path "$(repo_dir "$repo")/Cargo.toml" "${packages[@]}"
    fi
  done
}

# Fails before anything is written when a source is missing.
preflight() {
  local row kind src dest missing=0
  for row in "${PLAN[@]}"; do
    IFS='|' read -r kind src dest <<<"$row"
    if [ "$kind" = bin ] && [ "$DRY" = 1 ]; then continue; fi
    if [ ! -f "$src" ]; then echo "missing: $src" >&2; missing=1; fi
  done
  [ "$missing" = 0 ] || { echo "nothing was installed" >&2; exit 1; }
}

sudo_step() {
  cat <<EOF

One step needs root, and is not run here. accountd reads its caller table from
/etc/porter/callers.toml (rows of \$XDG_CONFIG_HOME/porter/callers.toml win, but the system
file is where inferd.service is named a porter daemon). inferd reads who is calling from
/etc/porter/inferd-callers.toml alone: a [callers] table in inferd.toml is ignored. They read them
only when they start:

  sudo install -D -m644 $PORTER/dist/callers.toml /etc/porter/callers.toml
  sudo install -D -m644 $PORTER/dist/inferd-callers.toml /etc/porter/inferd-callers.toml

The agent daemons name their callers the same way, by the unit behind a connection, from
/etc/quire/<daemon>-callers.toml (the system's file alone can say which unit is sill, the shell;
rows of ~/.config/quire/<daemon>-callers.toml are laid over it, but never give that role). Read
at startup, too:

  sudo install -D -m644 $DOCKET/dist/intentd-callers.toml /etc/quire/intentd-callers.toml
  sudo install -D -m644 $DOCKET/dist/companiond-callers.toml /etc/quire/companiond-callers.toml
  sudo install -D -m644 $DOCKET/dist/readerd-callers.toml /etc/quire/readerd-callers.toml
  sudo install -D -m644 $DOCKET/dist/acp-callers.toml /etc/quire/acp-callers.toml
  sudo install -D -m644 $ALMANAC/dbus/memory-callers.toml /etc/quire/memory-callers.toml

Then:  systemctl --user daemon-reload
       systemctl --user restart accountd inferd

To add an AI key (inferd holds them, not accountd), with inferd stopped:
       systemctl --user stop inferd; inferd add <provider> --allow <app>; systemctl --user start inferd
EOF
}

# Do user units get a private view of the system here? Ubuntu 24.04 and later refuse it to programs
# without an AppArmor profile; a unit that asks for it then fails to start. The drop-ins are left
# out then (scripts/install.sh prints the remedy).
SANDBOX_ON=1
decide_sandbox() {
  case "${INSTALL_DEV_SANDBOX:-auto}" in
    on) return 0 ;;
    off) SANDBOX_ON=0; return 0 ;;
  esac
  command -v systemd-run >/dev/null 2>&1 || return 0
  systemctl --user show-environment >/dev/null 2>&1 || return 0
  systemd-run --user --quiet --collect -p PrivateTmp=yes -p ProtectSystem=strict --wait true >/dev/null 2>&1 || SANDBOX_ON=0
}

do_install() {
  load_manifest
  decide_sandbox
  build_plan
  say "install-dev: prefix $PREFIX, units in $UNITS"
  [ "$DRY" = 0 ] || say "(dry run: nothing is built or written)"
  build_all
  preflight
  local row kind src dest
  for row in "${PLAN[@]}"; do
    IFS='|' read -r kind src dest <<<"$row"
    install_file "$kind" "$src" "$dest"
  done
  drop_stale
  [ "$DRY" = 1 ] || write_manifest
  [ "$DRY" = 1 ] || say "manifest: $MANIFEST"
  if [ "$SANDBOX_ON" = 0 ]; then say "note: this system does not let user units have a private view of the files, so the sandbox drop-ins were left out; see scripts/install.sh --help"; fi
  if under_home; then say "note: ProtectHome is read-only (not yes) on companiond and readerd, because $BIN is under \$HOME"; fi
  sudo_step
}

do_uninstall() {
  [ -f "$MANIFEST" ] || { say "nothing to uninstall: no $MANIFEST"; return 0; }
  local path kind sha pass rows
  rows="$(sort -t$'\t' -k3 -r "$MANIFEST")"
  # Files first, then the manifest, then the directories this install made (deepest first, and
  # only when empty): what is left, an edited config or the person's own files, is theirs now.
  for pass in files dirs; do
    if [ "$pass" = dirs ] && [ "$DRY" = 0 ]; then rm -f "$MANIFEST"; fi
    while IFS=$'\t' read -r kind sha path; do
      [ -n "$kind" ] || continue
      if [ "$pass" = files ] && [ "$kind" = dir ]; then continue; fi
      if [ "$pass" = dirs ] && [ "$kind" != dir ]; then continue; fi
      if [ ! -e "$path" ]; then act "already gone: $path"; continue; fi
      case "$kind" in
        dir)
          if [ "$DRY" = 1 ]; then act "rmdir $path (if empty)"
          elif rmdir "$path" 2>/dev/null; then act "removed: $path/"
          else act "kept (not empty): $path"; fi ;;
        config)
          if [ "$(sha_of "$path")" != "$sha" ]; then act "kept (you edited it): $path"
          elif [ "$DRY" = 1 ]; then act "remove $path"
          else rm -f "$path"; act "removed: $path"; fi ;;
        *)
          if [ "$DRY" = 1 ]; then act "remove $path"
          else rm -f "$path"; act "removed: $path"; fi ;;
      esac
    done <<<"$rows"
  done
  say "Then:  systemctl --user daemon-reload   (and stop what is running: systemctl --user stop intentd companiond readerd memoryd inferd accountd)"
  say "Not removed: /etc/porter/callers.toml, /etc/porter/inferd-callers.toml and /etc/quire/*-callers.toml (sudo rm them), your accounts and keys, and ~/.local/state/quire."
}

if [ "$UNINSTALL" = 1 ]; then do_uninstall; else do_install; fi
