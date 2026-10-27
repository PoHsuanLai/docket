# Bringing up the agent services under a demo shell

`docket-bringup` starts inferd, memoryd, intentd and companiond on a bus that already exists (the nested
demo's private bus), so the companion pane answers. It is the acceptance `World`'s setup (same daemons, same
`env_clear` scratch environment, same fake proc root) with daemons that outlive the command. Logic:
`crates/docket-live/src/bringup`; the binary is a thin wrapper.

## Build and find it

```sh
cd ~/docket && cargo build -p docket-accept -p docket-live --bins   # the daemons must sit beside docket-bringup
~/docket/target/debug/docket-bringup ...                            # or: cargo run -p docket-live --bin docket-bringup -- ...
```

`cargo run --bin docket-bringup` alone does not build the sibling daemons (`accept-intentd`, `accept-companiond`);
`up` says so if they are missing. inferd and memoryd are the test-keys builds `build.rs` puts under
`target/<profile>/accept-siblings/`.

## Commands

```sh
docket-bringup up --bus <dbus address> --root <abs scratch dir> --shell-pid <pid> \
    [--keys-home <abs dir>] [--catalog <abs dir>] [--providers <abs dir>] [--inferd-config <abs file>]
docket-bringup status --root <dir>
docket-bringup down --root <dir>
```

`up` prints one line per daemon, `inferd 4121 ok`, in start order (inferd, memoryd, intentd, companiond), after
each has its bus name (`org.quire.Inference1`, `Memory1`, `Intents1`, `Companion1`). On any failure it stops
what it started and exits 1 (2 for a wrong command line). `status` prints `name pid running|gone`. `down`
prints `name pid stopped|killed|gone`, last started first, and leaves the root on disk.

- `--shell-pid` is the demo shell. The fake proc root (`<root>/proc`) puts it in `sill.service`, the unit
  memoryd gives the `sheet_host` role. Bus names give intentd and companiond their roles
  (`org.quire.Shell`), so the shell must own those names on `--bus`.
- Root layout: `data config cache run keys proc logs`, plus `procs`. Daemon logs: `<root>/logs/<name>.log`.
- `<root>/procs` has `name pid start-time` lines (field 22 of `/proc/<pid>/stat`). `down` signals only a pid
  whose start time still matches: SIGTERM, then SIGKILL after 5 s. Never by name.
- `--keys-home DIR` (a directory `dev/live/cloud-key.sh` filled; mode 0700): inferd uses `DIR/inferd.keys`
  and the home's non-secret records are copied into the root. The key file is never opened by this tool.
  Without it inferd has a scratch key file, cloud models are unavailable, and `up` prints the exact
  `env -i ... inferd add openrouter --allow org.quire.Companion --allow org.quire.Intents` line to add a key
  into this root. Stop the services first (`down`), run it in your own terminal (the key is typed at inferd's
  prompt), then `up` again.
- `--catalog DIR` copies the stoker catalogue entries (`*.toml`) in; cloud models need them.
- `--providers DIR` copies porter's provider files (`*.toml`, no secrets; an install keeps them in
  `~/.local/share/porter/providers`) into `<root>/data/porter/providers`. Without `--keys-home`, pass it before
  the `inferd add` step: inferd adds a key only for a provider it serves.
- `--inferd-config FILE` adds your own `[ai]` rows (data floors, model choices, caps; see
  `dev/live/inferd.cloud.example.toml`). Nothing is added by default, so no class of data may leave the computer
  until you say so. The file may not hold `[engines]`, `[probe]` or `[callers]`.

## Refusals

- `--root` inside a git checkout, `$HOME` itself, `/`, relative, or with `..`.
- `--bus` equal to (or sharing a socket with) this process's `DBUS_SESSION_BUS_ADDRESS` or
  `DBUS_SYSTEM_BUS_ADDRESS`, and the usual real sockets (`/run/user/<uid>/bus`, the system bus socket).
- `up` on a root whose recorded daemons are still running.

## No local model, ever

The `inferd.toml` it writes holds an empty `[engines]` (no program, nothing attached) and `[probe]` with no
Ollama, llama.cpp or LM Studio port. A local model on the 16 GB GPU once broke the display. A test asserts it.
