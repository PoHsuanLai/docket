# Conventions

docket follows the program's shared conventions, whose text lives in quire's `CONVENTIONS.md`
(identical in quire, sill, shell-host, palmrest and detent): types, traits, effects, errors,
tests, comments, change discipline, borrowing. Read it first. `ARCHITECTURE.md` here adds the
crate map, the one-home table, the traits, the recipes and the repo rules; where the two
disagree, `ARCHITECTURE.md` wins for docket. docket copies porter's repo shape and inherits
porter's additions below, which hold here for the same reasons.

What docket adds or decides differently, each with its reason:

1. **Closed sets are enums without `Word`.** docket does not depend on quire's `ds-core` (nothing does
   directly; `docket-ds`, the adapter quire apps use, reaches its vocabulary through `ds-intents`): the router and the agent must build for any
   desktop. A closed set's stable slug is its serde `snake_case` form (the same slug in files,
   on the bus and in the audit log); nothing hand-writes `slug` or `parse`. Where a list of
   every variant is needed (`CallerRole::ALL`, `Member::ALL`) it is written by hand and a test
   with a wildcard-free `match` stops the build when a variant is added.
2. **Async seams use `-> impl Future<Output = ...> + Send`**, so implementations write
   `async fn` and every future can cross a multi-threaded runtime. Closed sets of
   implementations are enums or generic parameters bundled in `docket_router::Seams`, never
   `dyn`. The router is generic over one parameter, the bundle.
3. **`todo!()` bodies exist only while an interface is frozen and its behaviour is not built.**
   Each one is listed in `FINDINGS.md` with the work that removes it. A test whose subject is a
   `todo!()` is `#[ignore = "<what closes it>"]` and carries real assertions.
4. **Pure crates reach no effect, and the portable core reaches no desktop.** `docket-core`, `policy-point`, `action-review`,
   `docket-router`, `companion-wire`, `agent-loop`, `voice-wire` and `voice-loop` never reach
   `tokio`, `zbus`, `pipewire`, an HTTP client, a database or the MCP SDK, whatever their
   dependencies' features; `scripts/check-boundary.sh` fails the gate if one does; `scripts/check-portable.sh` does the same for the portable set (ARCHITECTURE.md section 1a) with `--no-default-features`. Only
   `policy-point` (and what runs the router above it) reaches `cedar-policy`; only
   `actions-mcp` reaches `rmcp`. `docket-core` never reaches `toml`: manifest TOML is parsed in
   `docket-router::registry`.
5. **docket sits below `cua` and `sill`.** No crate of the computer-use repo or of sill enters
   any tree here, dev-dependencies included; the boundary script checks it.
6. **One message model.** Every exchange between agents, and between the person and an agent,
   is `prov::Message`. docket defines no second return, report or result type. What it adds is
   the unstamped `MessageDraft` a sender hands the router, `Delivery` (the router's answer) and
   `InboundLine` (what a receiver's planner may read). **A message carries no authority**: a
   request in it is evaluated under the receiver's own task policy and the whole gating
   pipeline, and the message's label joins into the receiver's taint.
7. **A model never gates and never reads what it may not.** Policy comes from the person's own
   words (`TaskPolicy`), is enforced by Cedar, and can only be tightened by a reviewer; a
   planner sees untrusted text only as a `Handle`; a refusal reaches a planner as a coarse
   `DenyCode`, never a policy id or a reviewer's words. Tests named for these guarantees
   (`tighten_never_loosens`, `planner_view_hides_untrusted_text`) are never weakened to pass.
8. **Step inputs and outputs are `Serialize + Eq`.** Every state, input and effect of an
   `agent-loop`, `voice-loop` or router machine can be written to a trace and replayed.
9. **Labels are literals until prov's lattice is filled.** `Label::{trusted_user, untrusted,
   join}` are frozen `todo!()`s in porter; docket's tests and fakes write `Label { .. }`
   literals, and every call to `join` is behind an `#[ignore]`d test.
10. **Nothing ambient below the daemons.** The clock (`docket_core::Clock`; the one system clock is `docket_store::SystemClock`), the apps, the
    sheet, memory, the reviewer, the reader and the grant store are passed in; only `intentd`,
    `companiond`, `readerd`, `voiced` and `actions-mcp` read the system clock, the environment
    or a bus.
11. **Proposed values are `AgentConfig`.** Every number the specs mark "proposed" is a field of
    `docket_core::AgentConfig` with its default and a row in `SETTING_ROWS`; a test pins the
    two together. The settings keys themselves belong to sill's Intelligence page.
12. **One integration-test executable per crate.** Cargo links every file directly under `tests/`
    into its own executable, and each one statically links the crate's whole dependency graph, so a
    build directory grows with the number of files. Integration tests are therefore modules of
    `tests/it/main.rs` (`mod <topic>;`), shared helpers are `tests/it/support/` modules
    (`use crate::support::...`), and goldens, schemas and fixtures stay beside them under `tests/`.
    A test that includes another crate's helper does it by `#[path]` into that crate's
    `tests/it/support/`. A separate target (`tests/<name>.rs` plus `[[test]]` in the crate's
    `Cargo.toml`, with a comment in `tests/it/main.rs`) needs a stated reason: it changes the
    environment (`set_var`, a panic hook, the current dir), holds a process-wide singleton the others
    must not share, needs its own `required-features`, has `harness = false`, or relies on being a
    separate process. A new `tests/*.rs` without that reason is a mistake. Select tests with
    `cargo test -p <crate> --test it <filter>`. Dependencies build without debug info
    (`[profile.dev.package."*"]` in `.cargo/config.toml`).
13. **Docket keeps no list of agents, and nothing about an agent is copied from the person's home.**
    Which agents exist, their versions and their digests come from the agent registry's snapshot
    (`docket-agents`); `agents.toml` holds only the person's choices (the registry id, the pinned
    version, the model, the way to sign in, the label). The network is reached by the install and
    update commands alone, never when an agent starts, and a newer version is installed only when
    the person names it. An agent signs in inside its own home under the agents directory, kept
    between runs; `--acp-credentials` (copying a login into a scratch home) is legacy, for the
    live eval of an agent that has no sign-in of its own. The model is the person's choice, sent
    once after the session opens, and only one the agent offers; switching a session's *mode* stays
    a destructive action through the normal sheet. Text a person sees never names the protocol
    ("sign in with Google", "Gemini 3.1 Pro"). What an agent offered (its models, its ways of signing
    in, whether the last start was signed in) is written to `state/<id>/offered.toml` whenever a
    session with it opens; the Settings app reads it (`docket_settings::read_agents`) and starts an
    agent only on an explicit refresh (`docket-agent PROGRAM --refresh`), never when a page opens.

- Where the assistant may run: a place with no row in the settings is Off, a value the reader cannot read is Off, and the privacy floor filters in `docket_settings::route` before any setting is looked at; a cloud place is never chosen unless its row says On or AskEachTime (owner decision 2026-10-09).

13. **Handles: one for every thing the view shows, the same number for the same thing, one way to name it.**
    A model fixes its behaviour through the view, the tools and the wire, never a per-model prompt.
    (a) Every thing the planner view shows (where the person is, the visible items, the selection,
    a thing a tool returned) has a handle of its own, of its kind, beside the handles of its words;
    the view shows it first (`"handle": 12`), and the router resolves it exactly as it resolves the
    thing's `{app, kind, key}` (the same checks, the app's label). (b) The same text (same words,
    label and source) and the same thing keep their handle for the whole task, across turns and
    context refreshes (`HandleTable::reveal`, `try_mint_text`, `mint_entity`); new things get new
    numbers, and a number is never reused. The context view holds at most `MAX_VIEW_TEXTS` texts: past
    it the oldest text the current view does not use is evicted, and naming it later is
    `unknown_handle`. (c) A tool's `target` names one way, `{"handle": n}`, with `n` constrained to the
    handles held for that kind when there are at most 32 (`Catalogue::declaration_for`), so a handle
    of the wrong kind cannot be written; with none held the schema still accepts `{app, kind, key}`.
    (d) The prompt says a handle once: the context shows its own handles, the "What you can name
    but not read" section lists only the rest (what, and which step returned it), and what a kind of
    thing is for is said once per kind, not once per handle. The router still enforces where a handle
    may flow; the view does not repeat it. (e) A `wrong_type` for `target` names the kind it needs
    and the handles held of it (numbers and kinds only, never content). (f) The reader's answer
    reads as the reader's: its history step is `companion.read.<task>` and names the handles it
    read, shown as `quire_read summarise #10 → #11 (the reader's answer, text)`, and its handle is
    listed as `#11 text (63 characters): the reader's summarise answer about #10` (task word and
    inputs, never content). When the reader holds a text answer the task has not shown
    (`PlannerView::shown`), the view says so from handle provenance alone, never an app's name:
    `Ready to show: #11 (quire_finish {"show":[11]})`; an answer a done step took as an input (the
    body of a file that was made) is not offered. A step that can be undone ends `(can be undone)`,
    never with the undo id, which read as a handle. (g) A call is not run again when its answer
    is already held. The repeat guard (`agent-loop::guard`) holds a call whose effect is `Read`
    and that returned a handle, and an identical `quire_read` (same inputs, task and want), with
    `not run: you already hold its answer #n`; after `HOLDS_BEFORE_STOP_ANSWERED` (3) such holds
    the turn ends saying "I could not finish this." A call that may write, or whose effect is not
    known, is never held this way (a person may want two sends), and a call that wrote makes the
    held reads stale. The effect travels on `PlannedCall::effect`; this is the one guard, not a
    second mechanism beside it.

14. **A check script never pipes into `grep -q` under `pipefail`.** `grep -q` leaves at its first
    match, the writer dies of SIGPIPE, and the pipeline's status is the writer's, not grep's. A
    check written `cmd | grep -q x` then fails at random, and one written `if cmd | grep -q .` can
    read a hit as a miss and pass what it should fail. Match from a here-string
    (`grep -q x <<<"$rows"`) or test captured output (`[ -n "$(cmd)" ]`).
