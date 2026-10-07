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
10. **Nothing ambient below the daemons.** The clock (`docket_router::Clock`), the apps, the
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
