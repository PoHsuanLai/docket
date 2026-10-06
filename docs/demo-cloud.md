# Demo: the companion on a cloud model (OpenRouter)

For the owner, on this machine, with sill not running. Everything is installed for your user; the one
root step is a copy of one file. The companion talks to inferd, inferd to OpenRouter with a key that only
accountd holds. About ten minutes.

Run the first step from the checkouts the installer builds from: `~/docket`, `~/porter`, `~/almanac` and
`~/stoker` (the model catalogue inferd reads).

## 1. Build and install

```sh
cd ~/docket
dist/install-dev.sh            # cargo build --release --locked in three repos, then installs under ~/.local
```

`dist/install-dev.sh --dry-run` first if you want to see every file it writes. It prints one more step and
does not run it; run that yourself:

```sh
sudo install -D -m644 ~/porter/dist/callers.toml /etc/porter/callers.toml
```

(accountd's caller table. inferd.service is named a porter daemon there, which is what lets it fetch the
key. Rows in `~/.config/porter/callers.toml` win over it, but this is the file the dist ships.)

## 2. Reload systemd

```sh
systemctl --user daemon-reload
```

## 3. Add the key

You need an OpenRouter API key (openrouter.ai, Keys). accountd keeps it in the Secret Service (KWallet) and
checks it with one call to OpenRouter.

```sh
systemctl --user stop accountd.service
accountd add openrouter --allow org.quire.Companion --allow org.quire.Intents
# paste the key; echo is off
systemctl --user start accountd.service inferd.service
```

`org.quire.Companion` is the planner. `org.quire.Intents` is intentd's policy writer and reviewers, which
also ask inferd; without it a write cannot get a task policy (reads still work). Add `--allow org.quire.Reader`
if you will have it read mail.

## 4. Let prompts leave the computer

The defaults keep every class on this computer (`ai.local_only = "on"`, floors at `on_device`). The companion,
the policy writer and the reviewers all send the class `prompt`, and only that; readerd sends the class of what
it reads. Append this to `~/.config/quire/inferd.toml` (the installer put the `[callers]` tables there;
inferd rereads the file within a moment, no restart):

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
sudo rm /etc/porter/callers.toml
systemctl --user daemon-reload
```

`--uninstall` keeps `~/.config/quire/inferd.toml` because you edited it (delete it by hand), and does not touch your
account or key: remove the account in Settings later, or delete the OpenRouter key at openrouter.ai.
