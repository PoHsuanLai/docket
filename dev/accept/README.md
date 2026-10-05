# Agent-tier acceptance

The first end-to-end run of the agent tier with real daemon code in real processes on a private
bus. It proves SPEC section 5 flow (a) (prompt, plan, an Outbound call that asks, a Confirm1
answer, perform, journal, undo) and flow (c) (an injected mail body never reaches the planner;
the reader's output comes back as a handle; the send to an untrusted recipient still asks,
quoted).

## Where it runs

`crates/docket-accept/tests/flows.rs`, a test of the workspace: it runs in the gate
(`~/rs-wt/gate-lane-jailed.sh`, inside `~/desktop/harness/jail.sh`) with every other test.
`dev/accept/run.sh` runs only this package in the same jail, for working on it.
`ACCEPT_SHOW_PLANNER=1` prints every view the planner was shown.

## What is real

| Piece | How it runs |
|---|---|
| intentd, companiond, readerd | their own `run()`, one process each (`src/bin/accept-*.rs` are the packaged mains under names that cannot collide in one target dir), `env_clear`, scratch HOME and XDG dirs, the private bus |
| memoryd (almanac) | the packaged binary, built by `crates/docket-accept/build.rs` from `../almanac` with `--features test-keys` into `<target>/accept-siblings/almanac`; `env_clear`, scratch dirs, `MEMORYD_KEYS=file:<scratch>/keys/memoryd.keys` (no Secret Service on the bus) and `MEMORYD_SANDBOX=off` (Landlock blocks reading the callers' `/proc/<pid>/exe`; the jail is the isolation). Callers by executable path: the router is intentd's binary, the shell is the test process |
| inferd (porter) | the packaged binary, built the same way from `../porter`; `inferd.toml` in the scratch config names the replay engine `scripted` (the cassette) and the callers by executable (memoryd, intentd, companiond, readerd) |
| the bus | `docket-testbus`'s `dbus-daemon`, killed by PID on drop, with a watchdog if the test dies |

## What is scripted

The model is a cassette per test under `dev/accept/cassettes/` (JSON Lines, inferd's replay format):
`flow-a`, `flow-a-refused`, `first-use`, `flow-c`. Entries play in order and match on role (tools present
or absent) and on text (`contains`, `lacks`). The tests prove what a model was and was not shown through
the cassette: flow (c)'s planner entries carry `lacks` for the body and the summary, so a leaking view has
no answer (a replay miss, the answer ends Failed); the reader's entry needs the fence instruction and the
body in its request; `flow-a-refused`'s closing words need the planner to have been told
`mail.message.forward not confirmed`. Handle numbers in `flow-c` (the body is #1, the reader's answer #2)
come from the router's sequential handle table. inferd's audit trail (`World::model_turns`) counts turns
by app. The scripted `Inference1` is gone: nothing needed it. Not provable any more: the data class of the
reader's session (the audit entry has no class) and the planner's full view (`ACCEPT_SHOW_PLANNER` is gone).

| Piece | Why | File |
|---|---|---|
| `org.quire.Confirm1` (sill's sheet) | owns `org.quire.Confirm1` and `org.quire.Shell`, so intentd derives sill's `confirm`, `launcher` and `control` roles from `intentd.toml` as it would for sill; answers Allow (with a receipt) or Refuse from a queue the test fills | `src/confirm.rs` |
| `org.quire.Mail` provider | `org.quire.IntentProvider1` served with `docket_client::serve_on` for the manifest `fixtures/org.quire.Mail.toml`; holds a forward or send for an undo window like mailo | `src/provider.rs` |

## What mailo's real provider must match

The manifest `dev/accept/fixtures/org.quire.Mail.toml` is the contract the run proves:

- `mail.thread.search` (Read, `Entities(mail.thread)`), `mail.thread.read` (Read, text result with
  `trust = third_party(mail)`), `mail.contact.search` (Read, `Entities(mail.contact)`, trusted
  titles: the person's own address book), `mail.message.forward` (Outbound, `undo = "token"`, an
  entity `to` with the `recipient` sink, `dry_run = "preview"`), `mail.message.send` (Outbound,
  `to` text with the `recipient` sink, `body` with the `body` sink, `undo = "token"`).
- Entity-list results are labelled by the provider: thread lists `Label::untrusted(Mail, ..)`,
  contact lists trusted by the app. A `Preview::Message` from `DryRun` labels each recipient
  (`to`): the sheet draws a recipient quoted from mail when the label is untrusted.
- `Perform` of an Outbound action answers `Undoable::Yes(token)` valid for the send window and
  `Undo(token, actor)` cancels the held send; after the window `Undo` answers `UndoFault::Gone`.
- Every member but `Summon` refuses a caller that is not the owner of `org.quire.Intents1`.
- Action names carry the app prefix (`mail.`): the spec's `contacts.search` is
  `mail.contact.search`.

## Known gaps the run found

See FINDINGS.md, "f4-e2e acceptance". Finding 1 (the plan card and `NeedsYou(Confirm)`) is fixed and tested by
`flow_a_the_answer_shows_the_sheet_while_it_waits`.
