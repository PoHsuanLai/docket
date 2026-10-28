# Demo: the companion on a cloud model (OpenRouter)

For the owner, on this machine, with sill not running. Everything is installed for your user; the one
root step is a copy of one file. The companion talks to inferd, inferd to OpenRouter with a key that only
accountd holds (inferd asks it for the key when a turn needs it). About ten minutes.

Run the first step from the checkouts the installer builds from: `~/docket`, `~/porter`, `~/almanac` and
`~/stoker` (the model catalogue inferd reads).

## 1. Build and install

```sh
cd ~/docket
dist/install-dev.sh --cloud    # cargo build --release --locked in three repos, then installs under ~/.local
```

`--cloud` opens inferd's network (a drop-in, `inferd.service.d/cloud.conf`). It is opt-in because local
engines share inferd's sandbox, so an on-device-only setup keeps no network; without the flag inferd stays
offline and a hosted model cannot connect.

`dist/install-dev.sh --cloud --dry-run` first if you want to see every file it writes. It prints one more step and
does not run it; run that yourself:

```sh
sudo install -D -m644 ~/porter/dist/callers.toml /etc/porter/callers.toml
sudo install -D -m644 ~/porter/dist/inferd-callers.toml /etc/porter/inferd-callers.toml
```

(the caller tables. accountd's names the Settings app, the shell and inferd.service (the AI broker); rows in
`~/.config/porter/callers.toml` win over it. inferd reads who is calling from its own file alone, which only root
can write; a `[callers]` table in `inferd.toml` is ignored.) The daemons read the tables only when they start, so
the next step restarts them.

## 2. Reload systemd

```sh
systemctl --user daemon-reload
systemctl --user restart accountd inferd   # they read /etc/porter/callers.toml only at startup
```

## 3. Add the key

You need an OpenRouter API key (openrouter.ai, Keys). accountd keeps AI keys: it files the key in
the Secret Service (KWallet, under `porter.accountd`) and checks it with one call to OpenRouter. Each `--allow`
is a permit: the app named may use your AI keys. Typing the command is your consent. accountd reads its
accounts only when it starts, so it must be stopped while you add a key (the command refuses to run beside a
running accountd).

```sh
systemctl --user stop accountd.service
accountd add openrouter --allow org.quire.Companion --allow org.quire.Intents
# paste the key; echo is off
systemctl --user start accountd.service
```

`accountd` is in `~/.local/bin` (or the `--prefix` you gave the installer). Add `--class <class>` (repeatable)
to narrow a permit to some kinds of data; without it, every class is allowed, and `ai.floor` in step 4 still
decides which may leave the computer.

`org.quire.Companion` is the planner. `org.quire.Intents` is intentd's policy writer and reviewers, which
also ask inferd; without it a write cannot get a task policy (reads still work). Add `--allow org.quire.Reader`
if you will have it read mail.

## 4. Let prompts leave the computer

The defaults keep every class on this computer (`ai.local_only = "on"`, floors at `on_device`). The companion,
the policy writer and the reviewers all send the class `prompt`, and only that; readerd sends the class of what
it reads. Append this to `~/.config/quire/inferd.toml` (inferd rereads the file within a moment, no restart):

```toml
[ai]
local_only = "off"

[ai.floor]
prompt = "anywhere"
```

## 5. Pick a model

Same file, the model rows: `ai.model.llm.<tier>` is `auto`, `""` (the catalogue's choice) or `cloud/<entry id>`,
an entry of `~/.local/share/stoker/catalog` that your account reaches. The companion plans on `balanced`; the
policy writer and the quick reviewer use `fast`. Both must call tools, which these do:

```toml
[ai.model.llm]
balanced = "cloud/claude-haiku-4.5"
fast = "cloud/gemini-3.8-flash"
```

If Claude Haiku 4.5 is retired, pick another entry (its file in the catalogue says so).
Other entries with tools: `cloud/kimi-k3`, `cloud/gpt-6-luna`. `auto` lets inferd choose among what the
grants and floors allow.

## 6. Start the rest and ask

```sh
systemctl --user start memoryd.service intentd.service readerd.service companiond.service
quire-do ask "Hello. What can you do on this desktop?"
quire-do ask "What do you remember about me?"
```

Mail, calendar and files are not installed here: the companion's actions are memory and its own (starting a task,
loading a skill), plus mail if you install mailo (`~/mailo/dist/install.sh`). Reads run; anything that changes
something asks, and **`quire-do` cannot answer a confirmation** (only sill's sheet can). Try
`quire-do ask "Remember that I like green tea"`: it prints the plan and "Waiting for your confirmation", exits 8,
and nothing is saved. That is the gate working, not a failure.

`--json` prints the whole answer object; `--space <id>` picks the Space (default `desktop`).

## 7. Logs, and undoing it all

```sh
journalctl --user -u inferd -u companiond -u intentd -u accountd -u memoryd -f
systemctl --user status inferd companiond intentd
```

inferd's audit trail (which app asked which model, never the text) is `~/.local/state/quire/inferd/audit.jsonl`.

To undo:

```sh
systemctl --user stop companiond readerd intentd memoryd inferd accountd
cd ~/docket && dist/install-dev.sh --uninstall   # removes exactly what the install put down; keeps an edited config
sudo rm /etc/porter/callers.toml /etc/porter/inferd-callers.toml
systemctl --user daemon-reload
```

`--uninstall` keeps `~/.config/quire/inferd.toml` because you edited it (delete it by hand), and does not touch your
key: remove the AI account in Settings later (accountd's list under `~/.local/state/porter`, and the
`porter.accountd` entry in KWallet), or delete the OpenRouter key at openrouter.ai.
