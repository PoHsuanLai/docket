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
| memoryd (almanac) | almanac's library, one process, the same SQLCipher logs and sealed files; **in-memory keys** (the real daemon needs a Secret Service, which the private bus does not have) and **no Landlock** (`src/bin/accept-memoryd.rs`). Callers by executable path, as in the real daemon: the router is intentd's binary, the shell is the test process |
| the bus | `docket-testbus`'s `dbus-daemon`, killed by PID on drop, with a watchdog if the test dies |

## What is scripted

| Piece | Why | File |
|---|---|---|
| `org.quire.Inference1` (the model) | inferd has no replay engine: its own tests put fake OpenAI-compatible engines behind it, which needs a spawned engine process. This speaks the same wire (`Open` returns a socketpair, `ClientFrame` in, `InferEvent` out), so porter-client's own D-Bus transport runs unchanged in every daemon. Roles are told apart by what they ask for: tools mean the planner, the fixed instructions tell the policy writer and the reader apart | `src/inferd.rs` |
| `org.quire.Confirm1` (sill's sheet) | owns `org.quire.Confirm1` and `org.quire.Shell`, so intentd derives sill's `confirm`, `launcher` and `control` roles from `intentd.toml` as it would for sill; answers Allow (with a receipt) or Refuse from a queue the test fills | `src/confirm.rs` |
| `org.quire.Mail` provider | `org.quire.IntentProvider1` served with `docket_client::serve_on` for the manifest `fixtures/org.quire.Mail.toml`; holds a forward or send for an undo window like mailo | `src/provider.rs` |

## What mailo's real provider must match

The manifest `dev/accept/fixtures/org.quire.Mail.toml` is the contract the run proves:

- `mail.thread.find` (Read, `Entities(mail.thread)`), `mail.thread.read` (Read, text result with
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

See FINDINGS.md, "f4-e2e acceptance". The ignored test `flow_a_the_answer_shows_the_sheet_while_it_waits`
holds the first one.
