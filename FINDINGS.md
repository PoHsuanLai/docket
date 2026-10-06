# Findings

Open items and standing facts. An entry names the condition that closes it. After fill wave 1,
the docket amendment, the intentd bus fill (F3), the w4-companion fill (the persistent companion,
the MCP edge), the w4-docket fill (the audit trail, the built-in providers, the inferd bodies, the
signals), the w5-docket fill (the reader's session, the gate's watch, the companion's asks, the
daemons' binaries) and the f4-docket fill (the MCP binary, the watched perform, the resolver seam) there is **1 `todo!()` body** in library and daemon code (the voice wave's v-voiced lane filled the other: `voiced::serve`), listed below; the tests contain
none, and no test is `#[ignore]`d. (The freeze had 37: `agent_step` was filled in fill wave 1; F3 filled
`DbusTransport::call`, `docket_client::serve`, `DbusLink`, `SheetConfirmer`, `FileGrants` and intentd's
`serve` and `main`; w4-companion filled companiond and `McpEdge::call`; w4-docket filled `record_of`,
`InferdModel::{chat, embed}`, `InferdWriter::derive`, `ReaderClient::extract`, both providers' `perform`,
`reader_request`, `ReaderService::{extract, extract_in}` and readerd's `serve`. w5-docket filled no stub: it
changed signatures the asks named and filled the two daemons' `main`.)

## Stubs behind frozen interfaces

| Where | Count | Closes when |
| --- | --- | --- |
| docket-ds `DsContextSource::snapshot` | 1 | fill wave 2 (quire apps): `ContextModel` to `Here`, `Selection`, `Visible` through `entity_ref`; a private window reports the app alone; password and PIN fields are never reported |

Total: 1. (voiced `serve` and the voiced binary closed in v-voiced, below: no stub is left in voiced.)

## Ignored tests

One, a documented gap rather than a wait for a fill: `docket-accept/tests/flows.rs`
`flow_a_the_answer_shows_the_sheet_while_it_waits` (f4-e2e finding 1). Every other test runs.

## Built at the freeze (pinned by tests)

docket-core: ids and their grammars, `validate` (a table, one row per `ManifestError` rule), `fits`,
`conforms`, `ValueSchema::shape`, `charge` and `halted`, `LeadText`, `Roster::seen_from`,
`TaskStart::from_args`, `skeleton_of` and `close` (a finished task's trusted episode skeleton), the
shipped manifests (`manifests/`), `AgentConfig` against `SETTING_ROWS`, every wire and stored type
(round trips and pinned JSON). policy-point: the schema and default policies load and validate
strictly; the two named hard rules are present by id. action-review: `plan`, `escalate`, `tighten`
(exhaustive over every ruling and every combination of verdicts and errors), `ReviewerSet::check`,
`verdict_shape`, the breaker (`note`, `repeated`: the five tests of the addendum). docket-router:
the role table (`permits`, `acting_role`), `gate` (the fixed order), `session_step`, `index_step`,
the undo journal, the registry, the handle table and the planner's context view (untrusted text is
a handle and appears nowhere in the serialised view), message stamping and delivery views.
companion-wire: every body round trips; the session records. agent-loop: `assemble` (budgets per
section, masking, deterministic), `choose_tier`, `front_step`, `completion_line`, `side_step`,
`idle_step`, `rebuild`. docket-dbus: the five XML files equal the skeletons' introspection; member
lists; the options dictionary on every call that starts work; broadcasts carry no content.
docket-client: `Intents` over a scripted transport. docket-fake and docket-eval: fixtures and fakes,
the corpus loads and every case meets its expectation, `wilson` and `judge` tables. actions-mcp: tool names, hints, offered
actions, `mcp_label`. intentd: the configuration, the built-in manifests, record kinds, the queue,
the memory seam, the clock, every unit file's sandbox lines. readerd: the fixed instructions, the
reader host. docket-ds: every mark conversion. voice-wire, voice-loop, voiced: see their rows in
`ARCHITECTURE.md` (the utterance and speech machines, the sentencer, the buffer, the introspection).

Filled in fill wave 1 (router): docket-core `compare`, `covers` and `intersection` (a policy against
another, a call against a policy, a child against its parent) with the whole-label and
whole-segment pattern match, and `tool_schema` (one property per parameter, a handle accepted where
text, an entity or a file goes). docket-router: `call_step` (every row of the lifecycle table),
`Router::handle` (all 32 members: the 29 of the freeze, `Run.DryRun`, and the control centre's `Control.TerminalGrants` and `Control.RevokeTerminalGrant`), `child_policy`, `roster_of`, the labels the router derives
itself (a model's value is trusted only when it traces to the person's turns, a thing the router
showed the session, or a closed set), consent per data class (a tainted session cannot lean on an
`Always` for a write), coverage, the task policy (derived from the person's turns, capped by a parent
and by a prompt field, widened only on confirmation), review, preview, confirmation, dispatch, the
journal, the breaker, halts that withdraw sheets, messages (stamped, labelled, delivered as input),
the shadow index, and the computer-use gate. docket-fake: `clear` and `add_*` on the apps and the
scripted seams, so one harness serves many cases. docket-eval: `run_case` and `run_corpus` over the
fake router with a judge that always allows and the widest policy.

## Router: open items and interface asks

Closed by the docket amendment (interface-asks 38 to 41, 44): the Cedar grid now follows
QUESTIONS S1 (an outbound act asks under `Default` and `AskMore`; the two-reviewer auto-run is
`TrustMore` only; the grid test and two corpus cases pin it), `Perform` and `.Run.Perform` carry
`session: Option<SessionId>`, `covers(policy, decl, call, labels)` reads the effect, the cap and
each argument's declared sink, `call_step` takes the built sheet in `Previewed` and the
parameter in `ArgsChecked` and `AppLink::perform` can say `AppFault::TimedOut`, and
`Context.Current` names the app. Open:

1. **`roster_of` has no `HandleTable`.** A planner's goal is untrusted text, so `TaskRecord` holds
   the `Reveal<String>` made when the task started (a handle in the spawning session's table).
   Ask: none; a handle minted into the reading session's table needs `roster_of` to take it.
2. **Stored state grew.** `SessionRecord`, `RouterState` and `TaskRecord` gained fields (the
   ledger, the breaker, what the session saw, its turns and policy, per-Space strictness, the
   shadow index, the goal and last step of a task). They are built by `new` and nothing outside
   docket-router constructs one by literal.
3. **`tool_schema` keeps its entity and date parts hand-written** (asks 43 and 138, decided in
   f4-docket). The leaves (text, integer, choice) go through stoker's `Shape`. Entities stay
   `{app, kind (const), key}` with `{ "handle": n }` as the alternative, because the planner's
   grammar and MCP read JSON Schema, which says `const` and `format: uri` and `Shape` (even with
   `OrHandle`) does not; dates stay the `{year, month, day}` record `args_from_json` already checks
   (a day in its month), because a second string form would be a second parse path for a model to
   get wrong. Both are pinned by `tool_schema_snapshot`. Nothing is asked of stoker.
4. **A target's Space comes from a resolver** (ask 42, closed in f4-docket). `prov::EntityId` is
   unchanged; `SpaceOf` (`docket-router::spacing`) answers the Space scope of an entity, and the
   router's one resolver is its shadow index (the scope each app pushed with the entity).
   `relation_of` makes a call `Other` when any target is scoped to a Space that is not the
   session's, `Unbound` for an action on nothing and `Same` otherwise (an entity the index does
   not know is `Same`: the app refuses a thing it does not have). Cedar's `cross-space` rule then
   asks the person even for a read, and `mcp-other-space` denies an MCP client. Pinned by
   `docket-router/tests/crossspace.rs` (`cross_space_target_asks` and two more).
5. **Reviewer requests use `DataClass::Prompt`** (asks 16 and 61, closed): the person's own words and
   what a reviewer quotes of them. Its floor in `Policy::proposed` is this computer
   (`ai.floor.prompt`), so a reviewer request is pinned on-device; `action-review/tests/infer.rs`
   (`every_reviewer_request_carries_the_prompt_class_pinned_on_device`) holds every stage to it. The
   test lives in action-review rather than policy-point because policy-point (the Cedar grid) has
   no porter-infer edge and none is added for a test.
6. **The reviewer's schema is hand-written** (ask 14) until stoker's `Shape::to_json_schema` is
   filled; `InferReviewer` then switches to `verdict_shape(stage)`.
7. **Stage deadlines come from the clock.** `Clock::after(Millis)` is the router's timer: it races
   every `Reviewer::review`. `SystemClock` sleeps on tokio; the fake `FixedClock` completes only
   for `Millis(0)`, so a test that wants a timeout gives the stage no time and a reviewer that
   hangs (`ScriptedReviewer::hanging`).

## The terminal: `quire-do` (cli.md)

Built, with tests against `docket-fake` (the router in process) and a private bus:

- **Caller and rows.** `CallerRole::Cli`, `Origin::Cli`, `GrantCaller::Cli`, porter `prov`'s `Actor::Cli`,
  `ActorKind::Cli` and `Source::Cli` (porter branch `l-cli`; `Actor` and `ActorKind` live in `prov`, so the
  porter change is three variants, not one). A terminal's arguments are `Untrusted, Source::Cli`, derived by
  the router. The Cedar rows (`policy/default.cedar`, `policy-point/tests/terminal.rs`): a read is final; an
  undoable, outbound or destructive act asks and never goes to review, in every strictness; an ask-always
  action asks; a hidden action is denied; a destructive act always asks. `AskReason::FromTerminal` names it.
- **The standing grant.** The sheet for a terminal call that only the terminal rule asks about offers
  `ConfirmOffer::OnceOrFromTerminal` ("Allow from the terminal: this action, until logout"); the answer is
  `ConfirmAnswer::AllowedFromTerminal`. The router keeps it as `ActionMatch::One` in the task policy of the
  terminal's session (`docket-router::terminal`), shown by `Control.TerminalGrants`, revoked by
  `Control.RevokeTerminalGrant`, ended with the session (`Router::end_terminal_sessions`, which the daemon
  calls at logout). It lifts only the terminal rule: Rule of Two, an untrusted recipient or destination
  (everything typed in a terminal is untrusted, so an outbound send with a recipient never gets the offer),
  mass, ask-always and lasting-memory rules still ask, a destructive act is never offered it, and a sheet that
  answers it for any other caller records nothing. There is no flag, option or environment variable for it.
- **`Run.DryRun`** (a new `Intents1` member, `IntentsRequest::DryRun`): the app's preview through the same
  arguments check, labels, consent and policy as `Perform`, asking nobody and charging nothing; what the gate
  would refuse is refused with the same refusal.
- **`quire-do`** (`docket-cli`): commands, parameter mapping, `--dry-run`, `--json` (automatic when stdout is not
  a terminal), stdin `-`, `describe` through `Shape::to_json_schema`, the exit-code table, `__complete` and the
  completion files in `dist/completions`. `quire-do undo` and `undo --last` reach only the rows of the
  terminal's own acts (the router cuts `Run.Undo` and `Control.Journal` for the role).
- **Conformance.** `docket-eval --check-app <dir>` and `scripts/check-intents.sh` fail an app that ships a
  `.desktop` file and no valid intents manifest, and a menu command or shortcut in `<AppName>.ui.toml` that
  names no action and is not UI-only with a reason.

Open, and the asks they make (nothing below was edited in the other repos):

1. **Closed by F3 (interface-asks 84, 89, 95):** `DbusTransport::call` and intentd's `serve` and `main` are built,
   so `quire-do` reaches a real intentd on a D-Bus session bus (see "The bus path" below).
2. **Closed by F3:** intentd derives the caller from the connection (a `quire-do` process owns no bus name, so
   it is named by its executable: `org.quire.Do`, role cli), calls `Router::end_terminal_sessions` when logind
   removes the person's session, and serves `Run.DryRun`, `Control.TerminalGrants` and
   `Control.RevokeTerminalGrant`.
3. **sill** (ask): draw `ConfirmOffer::OnceOrFromTerminal` as a second button, "Allow from the terminal: this
   action, until logout", answering `ConfirmAnswer::AllowedFromTerminal`; list the grants in the control centre
   through `Control.TerminalGrants` with a revoke through `Control.RevokeTerminalGrant`.
4. **The launcher's natural-date parser** is sill's and docket cannot reach it, so a Date or DateTime parameter
   takes RFC 3339 only and says so. Ask: the parser as a pure crate below sill (quire or porter), which
   `docket-cli::when` then calls ("tomorrow at nine").
5. **ds menu commands and shortcuts carry no action id** (`ds::MenuItem::Item` has a `value: T` and a
   `Shortcut`), so rule three of cli.md section 6 cannot be read from the data an app has today. The shape
   `docket-eval --check-app` enforces the moment an app ships it is `<AppName>.ui.toml` beside the manifest
   (documented in `docket-eval/src/ui.rs`): `[[commands]] id, source = "menu" | "shortcut", chord?, action?` for
   every menu item and every shortcut binding, `[[ui_only]] id, reason` for the ones that are not an action.
   Ask (quire, `ds`): `MenuItem::Item` and the shortcut table gain `action: Option<ActionName>` (or
   `UiOnly(reason)`), and the app's build writes the file from its menu bar and shortcut table (a test that
   walks `MenuBarModel` and the table). Until then the check passes an app with a manifest and prints a note.
   Today `mailo`, `detent` and `sill` fail the check (a `.desktop` file, no manifest); `anyview` ships no
   `.desktop` file yet.
6. **A paused terminal session (ask 89), decided in F3: `Control.Resume` from the control centre.** The breaker
   pauses a session "until the person speaks", and a terminal has no turn to record. `Control.Resume` (the
   `control` role's alone) now also reopens every terminal session the breaker paused in the scope, with a fresh
   breaker (`Router::resume_terminals`, tests in `docket-router/tests/terminal.rs` and the end-to-end test). The
   alternative, a reset on the next `quire-do` after a cool-down, was rejected: it lets the very process the
   breaker stopped go on by waiting, and each round costs the person a sheet. A paused terminal says "paused: too
   many refusals in a row; the person has to resume it" (exit 7). Ask (sill): a "Resume the terminal" button
   in the control centre that calls `Control.Resume`; until it exists the person's ways back are `systemctl
   --user restart intentd` or logging out.
7. **The terminal's session is in the `desktop` Space** (the implicit session of every role without a session
   of its own), so a terminal call's target counts as `Same` (ask 42 stands).
8. **`--session`** is passed to `Run.Perform` and ignored by the router for every role but the companion.
9. **`quire-do <app> context`** shows another party's words as handles (`#<n>`, held by the terminal's session
   and usable as an argument by a later command); `Session.Display` is not open to the `cli` role, so a person
   at a terminal cannot read a held title. Ask: say whether the terminal may display what it holds.

## The bus path: `quire-do` to intentd to an app (F3)

Built and tested on a private `dbus-daemon` (nothing of the real session is named):

- **`DbusTransport::call`** (`docket-client/src/bus.rs`): one arm per `IntentsRequest` (all 32 members). Bodies
  are the JSON of the typed value the `docket-dbus` proxy documents. A request intentd refuses before it is a
  call (`NotAllowed`, `NoSuchSession`, `Malformed`) is the bus error `org.quire.Intents1.Error.<Variant>` and
  comes back as `IntentsReply::Refused`; a refused *call* is an answer: the body of `Run.DryRun`, `Run.Preview`,
  `Run.Suggest` and `Context.Current` is `Result<T, CallRefusal>`, and `Message.Send`'s is `Result<Delivery,
  SendRefusal>`. Six members answer a Request object (`Run.Perform`, `Run.Undo`, `Run.UndoAll`,
  `Session.Widen`, `Gate.Grant`, `Gate.Check`): the answer is the `org.quire.Intents1.Request.Response(code, body)`
  signal at the returned path, sent to the caller alone; code 0 carries the typed answer, code 2 a `WireRefusal`.
  The signal is subscribed to before the call (`docket_client::requested`), counts only from the owner of
  intentd's name, and the wait ends `Closed` when intentd leaves the bus. `DbusTransport::connect` does not ask
  the bus to activate a name that already has an owner.
- **intentd's side** (`intentd/src/bus/`): one struct per interface with the signatures of `docket-dbus`'s
  skeletons (a test holds the served introspection to `dbus/org.quire.Intents1.xml`; no member may carry a doc
  comment, zbus copies it into the XML). `serve_on(connection, router, config)` exports them and claims the
  name with `DoNotQueue` (a second intentd stops); `serve(router, config)` is the same on the session bus. `tests/bus_members.rs` sends every member over the bus and in process and requires the
  same reply.
- **Identity** (`intentd/src/peer.rs`): from the bus's own credentials for the connection: the same user, the
  well-known names it owns, and for a process that owns none, the cgroup behind its pid (f4-docket-ident: a
  terminal's child is `org.quire.Do`, see "f4-docket-ident" below; `/proc/<pid>/exe` is no longer read). Roles are `intentd.toml`'s (`dist/intentd.toml` is the shipped default; a file in
  `$XDG_CONFIG_HOME/quire` replaces it whole). An unknown connection is `NotAllowed`. This is advisory on a
  desktop where every process runs as the person: the cli role asks for everything but a read, and the
  stronger binding (Flatpak, a systemd scope) is later.
- **Apps** (`DbusLink`): `IntentProvider1` on the app's own name, started by activation when absent, the
  owner checked to be the person's own process; latencies 250 ms (instant), 5 s (quick), 10 min (long, reports
  progress itself); a timeout is `AppFault::TimedOut`. `docket_client::serve_on` / `serve` serve a provider and
  answer only intentd (every member but `Summon`). The mail app of the tests is docket-fake's `FakeMail`
  behind it.
- **The sheet** (`SheetConfirmer`): `Confirm1` is asked only when its owner plays the `confirm` role; no sill, an
  untrusted owner, a sill that vanishes or never answers is a dismissal or an expiry, never an allow. The
  Request object sill returns must send `org.quire.Intents1.Request.Response(0, <ConfirmAnswer JSON>)`.
- **Logout** (`intentd/src/logout.rs`): `SessionRemoved` of logind on the system bus ends the terminal sessions
  (the session of `XDG_SESSION_ID`, any session when it is unknown); the unit is `PartOf=graphical-session.target`.
  Tested against a fake logind on the private bus.
- **Files:** `FileGrants` (atomic, a damaged file is no grants), the manifests of `$XDG_DATA_HOME` and
  `$XDG_DATA_DIRS` (`quire/intents/*.toml`, the first directory wins), `dist/intentd.service`, `dist/intentd.toml`.
- **`quire-do` end to end** (`docket-cli/tests/e2e.rs`): the real binary, intentd's `start`, a fake mail provider,
  a fake sill `Confirm1`: a read exits 0, a write asks and the answer decides, "from the terminal" skips the next
  ask until the control centre revokes it, a hidden action exits 3, three refusals pause it (exit 7) until
  `Control.Resume`, `undo --last` reaches the app, and with intentd stopped every call exits 6.

By hand: `scripts/try-quire-do.sh` starts a scratch `dbus-daemon`, intentd, a fake mail app and a sheet that asks
on the terminal (`intentd/examples/try_apps.rs`), all with a scratch HOME and XDG directories, and prints the
`source` line for a second terminal where `quire-do` then works.

Built in the w4-docket fill (each with tests on a private bus, a scripted inferd or the fakes):

1. **The audit trail reaches memoryd.** `record_of` maps every `AuditRecord` to an almanac `Record` that memoryd
   admits (a message and an episode are almanac's own typed bodies, the rest `Area { Docket }` in their serde form
   with a call's targets as the `things` for cascade-forget; a record's Space is the one it names, else the Space of
   the call it reviews, else the session or task the router still holds, else `desktop`). `QueuedSink` is bounded
   (4096 records; the oldest are dropped and counted) and shared by clone; `AuditLog` writes one batch per Space as the
   router's `Caller`. **Degraded path:** while memoryd is away, locked or busy, what could not be written goes back in
   front of the queue in its original order, intentd says once that memoryd is not answering and once that it is back,
   and says how many records it had to drop; a record memoryd refuses for good is dropped and counted, never retried.
   `Setup.audit_every` (5 s) is the cadence. `Cause` is `None` on every record: a review names its call by id inside
   its payload, because the call's `EventRef` is memoryd's to make after the batch is written.
2. **intentd hosts `org.quire.Memory` and `org.quire.Companion`.** `HostedLink` (the `AppLink` of `SystemSeams`) answers
   those two names in process and every other app over D-Bus; their manifests are built in (`builtin_manifests`) and
   inserted before the installed ones, so `quire-do apps` lists them, and a file that declares either name is skipped
   with a reason. Memory: `recall` (a search of the session's Space, `RecallOver::Both`), `facts` and `propose`
   (`Staged`: memoryd labels the router's proposals untrusted, so they land pending), each result labelled with the join
   of the hits' labels; `memory.forget` is `Unsupported` (the router may plan a forget, only the shell's own UI may apply
   one, and the action is hidden). Companion: `Router::companion_perform` (docket-router, `companion.rs`): the
   spawner's goal is held as a handle in its own table and sent to the new worker as a request message, the worker is a
   child session under the spawner's policy (never wider), `AuditRecord::TaskStarted` is written, and a message to a
   task goes through `message_send`; only a companion session may call either.
3. **The models are over inferd.** `InferdModel::{chat, embed}` open one session per call with the need the request
   names (chat, structured output when it has a shape, tools when it has any, room for the prompt), its class and its
   tier, forward events to the sink and return the reply; every refusal and failure is a `ModelError`, so the reviewer
   asks the person. `InferdWriter::derive` shows the model the person's turns and the catalogue (class `Prompt`, tier
   Fast, JSON under a schema that names only catalogue actions) and believes nothing in the draft: an action must be in
   the catalogue, the ceiling is cut to what the chosen actions need, a recipient, destination or path is kept only if
   the person wrote it, the rationale is the person's own last words, and any failure is no policy. readerd:
   `reader_request` (the fixed instruction, the inputs as numbered data between a per-request fence the data cannot
   close, no tools, the strictest class of the inputs and, for unclassed text, the person's own words' floor, which is
   this computer), `answer_of` (the reply read back under the schema, entities only among those offered, `conforms`
   before anything is passed on), `ReaderService::{extract, extract_in}` (every handle resolved through
   `Session.Resolve`, the whole read fails if one cannot be) and `serve` / `serve_on` (`Reader1`, answering only the
   owner of intentd's name). `ReaderClient::extract_in` is the call from intentd; see the ask below for the seam.
4. **The signals.** `ManifestChanged`, `JournalChanged`, `BreakerTripped` and `Arrived` are the difference between two
   looks at the router's state (`Marks`, `changes`, every 200 ms; table-tested), so every way the state can change says
   so the same way; the manifest directories are rescanned every few looks (polling, since `notify` is outside the
   boundary) and a changed or removed file takes its app with it. `Hits` is the late part of a search: `Search.Query`
   over the bus answers the shadow index at once and the apps that hold kinds they do not index send their hits to the
   asker alone as `Hits` (not if the asker cancelled or asked again). `Halted` and `Resumed` are as before.
5. **A missing app is `AppFault::Unavailable`** (ask 105): `call_step` turns it into `CallRefusal::AppUnavailable`, and
   `quire-do` exits 6 (the end-to-end test installs the Files manifest and runs no Files app). **`serve(router, config)`**
   takes the configuration (ask 106).
6. **A computer-use run is rebuilt on restart** (ask 63): `companiond::recover` reads `cua.run.started`, `cua.asked`,
   `cua.confirmed`, `cua.taken_over`, `cua.handed_back` and `cua.run.finished` as `ReplayWhat::Run`, by the words of
   `cua-bus`'s `CuaRecord` (docket names no cua type); a run whose start is older than the window says nothing.
7. **`tool_schema` routes its text, integer and choice leaves through stoker's `Shape`** (ask 43); the snapshot is
   unchanged. Entities, handles, dates and the rest stay hand-written: `Shape` has no entity-or-handle form, and a date
   is `{year, month, day}` here against `Shape::Date`'s string, which would change the planner's grammar.

Not built yet, and what each blocks:

1. ~~`Reader::extract` carries no session~~: closed in w5-docket (see "The w5-docket fill").
2. **sill** must serve `Confirm1` and draw `OnceOrFromTerminal` (ask 3), and `quire-do` over a login needs the
   `Control.Resume` button (item 6 above).
3. ~~`Request.Proceed` answers `Malformed`~~: a watched `Gate.Check` implements it (w5-docket). On a request that is
   not a watched gate check it still answers `Malformed`: nothing waits for it.
4. **Identity beyond name and executable** (Flatpak, a systemd scope): done in f4-docket-ident (cgroup, porter's `ProcCallers`).

## The persistent companion and the MCP edge (w4-companion)

Filled: all of `companiond` (`PlannerModel::{request, plan}` and `converse`, `Companiond::{open, ask,
arrived, roster, tick}` and the entries beside them, `serve` and `serve_on`) and `actions-mcp`'s
`McpEdge::call` with its `ServerHandler`. Tested over docket-fake's router in process, a scripted planner
model and a private `dbus-daemon`; nothing real is touched.

- **One identity, many tasks.** Each task is a session the companion opened (`TaskRuntime`: turns,
  steps, handles, inbox, answer); `agent_step` is the only decision, `drive` carries the effects out (the
  planner, `Run.Perform` with the task's session, `Session.Read`, `Session.Close`). The front pointer is
  `front_step`; a finished task takes no follow-up (the next ask opens a new session and is shown the
  last one's episode).
- **The working set** is rebuilt for every planner turn (`sources`, then `assemble`): action cards from
  the installed manifests, recall by `Session.Recall` (`Inject` with the `recall` token budget,
  `TrustedOnly`, then `Recent` of `companion.episode` older than this run), the context, the roster
  without the task itself, this run's recent episodes, the inbox, and the task. Untrusted text is a
  handle because the router made it one. `render` puts the rules and the pinned profile in one system
  message and the sections in the assembler's order in one user message, with no clock and no ids, so
  the prefix is the same bytes while a task grows (`the_planner_prompt_keeps_its_prefix_while_a_task_grows`).
- **Episodes.** The router writes a task's skeleton at close. A worker's final report ends its task in
  the router first, so companiond hands the router the skeleton (`Session.Note`) in that case. The
  narrative is the idle pass: after 30 s of quiet, `Usage::Background`, no tools, from the skeleton text
  alone (even for a task that read untrusted text, whose narrative would otherwise be the reader's job),
  recorded as a second event labelled `Untrusted`, `Source::Model(Consolidator)`, private to the Space;
  an interactive request drops the stream (`Shared::interrupt`) and the job goes back to the queue.
- **Subagents, one message model.** A worker is opened with `Session.Open` (parent set, policy never
  wider), its goal goes as a `Request` message from its parent, it runs to its end inside the spawning
  call, and its final word is a `Report` message in the request's thread. Reports from workers and
  runs become a `CompletionNote` (a typed line, never the worker's words) and a roster line; a failure
  makes the orb wait. The person's own message to a subagent is a trusted event: the roster line quotes
  the first 80 characters at once, the conversation is tracked, and two idle minutes or a closed row
  write a side episode of their words verbatim. A message across Spaces is `Companiond::message`; the
  router labels it with what the sender read, the receiver plans on it as input and every call it then
  makes is gated by its own policy (`a_request_in_a_message_is_still_gated_by_the_receivers_own_policy`);
  the roster shows another Space's agents as presence only.
- **Serving.** `Roster()` and `Front()` read `Shared` (what the loop last wrote), so they answer while a
  planner turn waits on a confirmation; `Ask` answers the answer object's path at once and runs the loop
  behind it; answer objects have `View`, `Updated` and `Cancel`; `Act` answers `NotSupported` (the loop
  produces text and refusals, no cards). The served interfaces equal `dbus/org.quire.Companion1.xml`.
- **Additive change in a frozen crate:** `agent_loop::LoopInput::Messaged` (a request landed in an idle
  task's inbox: plan, with no turn of the person's). docket-client gained `Intents::{session_recall,
  session_read, session_note, session_task_policy}` in a new file (`ask` became `pub(crate)`).
  `Companiond` gained fields and `Companiond::new`; `ServeFault` gained variants; `serve` has a sibling
  `serve_on(connection, companion)`.

`actions-mcp`: `McpEdge::call` finds the tool among the offered actions (hidden and ask-always are not
tools), reads the JSON arguments by declared type (`read_call`, with a `target` key per the action's `on`),
labels each `mcp_label(client)`, performs it as `Origin::Mcp` and answers `{said, value}` or a coarse
`McpFault::Refused`; `ServerHandler` serves `list_tools` and `call_tool`. `McpAccess` is `Off` by default:
an off edge lists nothing and refuses every call. Tested with an rmcp client in process.

Open, and the asks they make (nothing was edited in the other crates beyond the additions above). **Asks 1 to 12 were taken up in the w5-docket fill (next section); the text stays as it was written.**

1. **A turn's text** (ask): `Companion1.Ask` carries a `TurnId` only and the router lets nothing read a
   turn back (`Session.Turn` is the launcher's). companiond learns the words from `Companiond::heard`,
   which nothing on the bus calls yet. Ask: `AskWire.turn: UserTurn` and `keep: ContextKeep`, or a
   companion-role `Session.Turns(session)`.
2. **The summoning app** (ask): `Context.Current` needs the app; `AskWire` has an opaque `WindowKey`.
   Ask: `AskWire.app: Option<AppName>`. Until then `Companiond::summoned_from(app)`.
3. **Session records have no write path** (ask): `Session.Note` takes an `Episode` only, so nothing writes
   `companion.session.*` and `recover` finds only messages and episodes. Ask: `NoteAsk` becomes
   `Episode(Episode) | Record(SessionRecord)`, stored as `Area { Companion }` with the kind
   `companion.session.<slug>`.
4. **Recent has no bodies** (ask): `RecentLine` carries no body or label, so the `RecentSource` that
   `recover` reads cannot be built over the router, and `RecallView::Episodes` is never produced (older
   days' episodes reach the planner as recalled text, not as `EpisodeLine`s with ids and outcomes). Ask:
   `RecentLine.body: Option<JsonText>` for entries the label allows, and the router building
   `RecallView::Episodes` from episode bodies.
5. **Primer, profile and the digest have no read** (ask): section 2 is empty. Ask: `RecallAsk::Primer` and
   `Profile` (or a field on the recall reply).
6. **The worker's session** (ask): the built-in provider answers the task id; acting as the child needs
   its session. companiond therefore carries out `companion.task.start` and `companion.task.message`
   itself (`Session.Open`, `Message.Send`) and does not send them to `Run.Perform`, so they are neither
   budgeted nor audited as a `Call`. Ask: the provider answers `{task, session}` (and companiond adopts
   it), or opening is left to companiond and the provider only records `TaskStarted`.
7. **An inbox line does not say who it is for** (ask): `Message.Inbox(Companion)` drains every companion
   session at once. companiond places a line by Space and crossing (the front task first). Ask:
   `InboundLine.to: Address`.
8. **Undo ids and handle sizes** (ask): the outcome the companion gets carries the app's token, not the
   journal's `UndoId`, and no handle size, so a step has no `undo #n` and a handle card says 0
   characters. Ask: the presented outcome carries both.
9. **`ActionCard` has no `on`** (ask): the planner's tool schema needs the target. companiond keeps the
   declarations (`Catalogue`) and adds a `target` key. Ask: `ActionCard.on: TargetKind`, and one
   `args_from_json(decl, json)` in docket-core beside `tool_schema` (companiond's `read_call` and
   actions-mcp's are two copies).
10. **A narrative should name its episode** (ask): `Episode::narrates` needs the router's own skeleton;
    companiond writes the second event from its own ledger (same words and steps, no undo ids). Ask: a
    narrative `Note` names the episode id and carries the `Narrative` alone; the router merges it.
11. **A subagent cannot be narrowed from here** (ask): a goal change in a side conversation makes
    `SideEffect::Rederive`, and `Session.Widen` only widens. Ask: a companion-role `Session.Narrow`.
12. **A run's inbox is cuad's** (ask): companiond cannot read what the person told a computer-use run, so
    `Companiond::told(agent, space, turn)` is its entry. Ask: the router copies person-to-subagent
    messages into the companion's inbox as notes, or `Companion1` gains `Told`.
13. **Scope left:** a worker runs inside the call that started it (a watch or background task needs a
    scheduler); `ProposeFacts` extracts nothing; the idle pass reads the skeleton, not the transcript (a
    reader client would run transcript narratives in readerd); `main` is still a skeleton; the planner
    opens an inferd session per turn (the engine's cache is by prefix, pinning is inferd's), and the
    MCP edge has no stdio or socket serve behind `McpAccess::On` and no `mcp.enabled` settings row (ask:
    the row, read by the hosting daemon). `McpFault` lost `Copy`.

Seams served: companiond to intentd (`Session.*`, `Run.Perform`, `Message.*`) and to inferd
(`Open` with `Need::Llm`), the shell and sill through `Companion1`; actions-mcp to intentd through
`Intents::perform`. Scenarios that prove them later (`~/rs-wt/integration/MAP.md`): the double-tap summon
that reads mail and answers with a handle; a follow-up task an hour later seeing the last episode; the
user redirecting a run and the front agent quoting them; two Spaces with a labelled message and a
presence-only roster; an MCP client listing tools and reading mail while a write asks.

## The w5-docket fill

Built, each with tests on a private bus, over the fakes or in process (nothing real is touched). Branch
`w5-docket`; the consumer call sites that change are listed at the end.

1. **`Session.Read` works in the running daemon (ask 126).** `Reader::extract(&self, session: &SessionId, ask,
   inputs)`: the router passes the session in `session_read`, `ReaderClient::extract` calls `Reader1.Extract(session,
   ask)`, `ReaderService::extract` ignores it (readerd resolves the handles itself), `ScriptedReader` records it
   (`sessions()`). `intentd/tests/reader_daemon.rs` runs the whole path on a private bus: a companion recalls untrusted
   text (a handle), `Session.Read` goes to intentd, to readerd and to a scripted inferd, and the answer comes back plain
   (an answer outside the schema, and a bus with no readerd, are refused). **Known limit (closed by the f4-docket fill, item 4):** `Session.Resolve` answers the
   text alone, so readerd cannot class it and sends it as the person's own words (`DataClass::Prompt`, pinned on this
   computer); the class of the handle's own label (mail) would pin it harder. Ask: `Session.Resolve` answers the
   handle's label too.
2. **A computer-use run has a session (ask 124) and its gate check can be watched (ask 123).** The cua role may
   `Session.Open` and `Session.Close` (its own sessions only) and only for an agent `Cua { run }`; a run has one session,
   whoever opened it. `Router::handle_watched(caller, request, Watch)` and `Watch` (`docket-router`: `progress`,
   `proceed` and `closed` callbacks, with the small std-only `Flag` and `Queue`): a gate check that must ask the person
   says `Progress(Confirming(id))`, waits for `Proceed` (10 s, then the step is refused unasked: `Unconfirmed(Expired)`;
   the 10 s is a setting since the f4-docket fill and is tested on a virtual clock) and only then draws the sheet; `Close` withdraws the
   request and its sheet (`Cancelled`), no `Response` follows. An unwatched check is exactly as before. On the bus the
   caller says it watches with the `watch` option of `Gate.Check` (`docket_dbus::OPTION_WATCH`), intentd's Request
   object answers `Proceed` and `Close` for the owner of the request only, and `Proceed` on a request nobody watches is
   still `Malformed`. The client: `Transport::watch` (default: the answer alone), `Intents::gate_check_watched(ask)` ->
   `GateWatch` (`next()` gives `GateEvent::Confirming(id)` then `Verdict(answer)`, `proceed()`, `close()`, and a cloneable
   `GateSteer` for another task); `InProcess` drives the router's future from `next()` (no runtime here). Tests:
   `docket-router/tests/watching.rs`, `intentd/tests/gate_watch.rs` (bus: told before the sheet, no sheet before
   `Proceed`, `Close` before and during a sheet, an unwatched check), `docket-fake/tests/fakes.rs`.
3. **The companion's asks (121), all but the scheduler and the settings row:**
   - `AskWire { session, turn: UserTurn, keep, parent_window, app }` (so a real `Companion1.Ask` works; `Companiond::heard`
     and `summoned_from` are gone, the summoning app is per task). companiond answers `Ask` and `Told` only to the
     connection that owns the shell's name (`CompaniondConfig.shell`, `org.quire.Shell` shipped), because the turn it is
     handed is the person's words and the router never recorded this copy. `Open` and `Close` are still open to any
     caller (see asks).
   - `NoteAsk` is `Episode(Episode) | Narrative { episode, narrative } | Record(SessionNote)`. `Record` is the serde JSON of
     a `companion-wire` `SessionRecord` under its slug: the router stores it as `AuditRecord::Session`, intentd as
     `Area { Companion }` of kind `companion.session.<slug>` (trusted, private to the session's Space), and companiond now
     writes `opened`, `asked`, `replied`, `finished` and `closed` records. `Episode` is refused for another Space or an
     untrusted skeleton. `Narrative` names the episode; the router merges it into its own skeleton from the task's ledger and
     records the narrated successor (`narrates` holds); the companion never restates a skeleton. A final report that ends
     a worker's task now leaves the episode at once in the router (companiond no longer hands it over), and a session
     nobody used (no turn, no step) leaves no episode.
   - `RecentLine { summary, effect, label, text, body }`: a body only for a trusted entry. `RouterRecent` (companiond) is the
     `RecentSource` over `Session.Recall`; `Companiond::restore(spaces)` reads each Space through a session it opens and
     closes, rebuilds, and `resume`s. `RecallAsk::{Episodes, Primer, Profile}` and `RecallView::{Primer, Profile}`: episodes
     come as `EpisodeLine`s (the skeleton as trusted lines, a narrative only by handle, the newest event of each id), the
     primer is cut to 200 lines, the profile is the facts the person stated themselves (`Actor::User`, trusted) at the desktop
     scope. companiond fills sections 2 and 4 of the working set from them.
   - `companion.task.start` answers a record `{task, session}` and companiond performs it through `Run.Perform` like any other
     call (gated, budgeted, audited), adopts the worker and runs it; the planner is told the task id. The manifest's result
     type stays `text` (there is no record `ParamType`). `docket_fake::host_companion(&router)` hosts the provider in the
     fakes. `companion.task.message` is still performed by companiond (`Message.Send`).
   - `InboundLine.to` (companiond places a line by where it was sent). `Undoable::Journaled(UndoId)`: the router answers its
     caller the journal's row instead of the app's token (apps still answer `Yes(token)`); `quire-do` reads it, companiond's
     steps carry it (`undo #n`). `Session.Handles` (companion role): the router's own `HandleCard`s with sizes, which replace
     the placeholders companiond kept.
   - `ActionCard.on`, and `docket_core::args_from_json(decl, arguments, label)` (with `ArgsFault`, `TargetFault`, `Why`
     moved from actions-mcp, which re-exports them): companiond's `read_call` and the MCP edge's are one reader. A list of
     entities with a handle among them is a list of values, as the companion wants.
   - `Session.Narrow(session, turn)` (companion role): the policy writer reads the person's words and the session's policy
     becomes the intersection with the one it has (with none, the parent's bound); a writer that fails changes nothing.
     companiond sends it for a subagent's side conversation (`SideEffect::Rederive`), workers only: a run's session is cuad's.
   - `Companion1.Told(agent, space, turn)` (the shell tells the companion what the person said to a subagent; companiond cannot
     read a run's inbox).
   - **`mcp.enabled` (ask, not done here):** quire's settings gain a row `mcp.enabled` (an on or off setting: off by default,
     QUESTIONS S7), read by the daemon that hosts the MCP edge (`actions-mcp`'s binary), which passes
     `McpAccess::On` to `McpEdge` and serves it over stdio or a socket; nothing was edited in quire.
4. **128:** `BreakerTripped` is emitted (intentd `signals`, `tests/signals.rs`: said once, again after a resume and a second
   pause): closed. `Resume(Only(space))` also lifts a global halt for that Space (the global halt becomes a halt of every other
   Space the router has a session in; `tests/control.rs`). The effect classes **"changes only the view"** and **"file with no
   undo"** cannot grow additively here: `Effect` is porter's `prov::Effect` (`Read < UndoableWrite < Outbound < Destructive`),
   ordered, and the Cedar grid, the budgets and the ceilings are written over it. Ask (porter): the two classes, with the grid rows
   and the ceilings decided by the person; until then a view-only action is `read` and a file with no undo is `destructive`.
5. **The daemons' `main`s.** `readerd::run` / `readerd::start` and `companiond::run` / `companiond::start` (configuration,
   `docket_dbus::session_connection` (shared with intentd), `DbusTransport`, `inferd_transport`, `restore`, `serve_on`),
   `CompaniondConfig` (`dist/companiond.toml`, every key optional: `shell`, `spaces`, `[agent]`). Binary tests on a private bus
   (`readerd/tests/binary.rs`, `companiond/tests/binary.rs`): the name is claimed, `Reader1` answers intentd alone and says
   unavailable with no router behind it, `Roster()` and `Front()` answer with nothing running, the configured shell is heard
   and another is not, a second daemon stops, and killing the daemon frees the name. `voiced` stays a skeleton.

Interface asks left after the w5 fill, as the f4-docket fill leaves them (the full account is in
"The f4-docket fill" below):

- porter: the two effect classes (item 4 of the w5 fill).
- quire: `mcp.enabled` (item 3 of the w5 fill): until the settings row exists the edge reads its own
  `actions-mcp.toml`.
- cuad: use `gate_check_watched`, open its run's session, and have `cua.run.start` answer the session it opened.
- almanac: let `Caller::Router` ask `MemoryRequest::Spaces` (see the f4-docket fill, item 5).
- A recent entry whose label is untrusted comes without a body (a message carrying what a worker read, a narrated episode):
  a restart falls back to the trusted skeleton events; nothing more can be rebuilt from them.

## The f4-docket fill

Branch `f4-docket`. Each item has tests on a private bus, in process, or over the fakes.

1. **`actions-mcp` is a binary** (stdio by default, `--socket PATH` for a long-lived socket, `--client NAME`).
   `McpConfig` (`$XDG_CONFIG_HOME/quire/actions-mcp.toml`, shipped as `dist/actions-mcp.toml`: `access = "off"`,
   `client`): off by default, so an edge nobody switched on lists nothing and refuses every call (it still runs,
   and says so on stderr). It claims `org.quire.ActionsMcp`, the name `intentd.toml` gives the `mcp` role, and a
   second edge stops (several clients share one edge through the socket; each connection gets its own `McpEdge`).
   The router names an MCP caller by that bus name, so consent grants are per edge process, while the `client`
   name labels the arguments (`Source::Mcp(client)`); per-client grants would need the client on the wire.
   `docket-dbus` and `zbus` join `actions-mcp`'s edges (`check-boundary.sh` rules updated). Tests
   (`actions-mcp/tests/binary.rs`, `env_clear`, private bus, intentd's `serve_on` over the fake router, an rmcp
   client on the child's pipes and on the socket): off lists nothing, on serves the registry and a read is allowed
   as the role, a second edge stops, two socket clients share one edge, the unit's command line parses.
2. **`Companion1.Open` and `Close` are the shell's** (the `Ask` check). `companiond/tests/serve.rs`.
3. **`Run.Perform` says `Progress`** to a watching caller: `Reviewing` (per stage), `Previewing`,
   `Confirming(id)` (told as the sheet is drawn, not waited on), `Dispatched`. `Watch::listening` is the
   listen-only watcher; `Run.Perform` takes the `watch` option (`Watching::Listening`: `Close` still aborts the
   call as for any request); the client has `Intents::perform_watched` -> `PerformWatch::next()` ->
   `PerformEvent::{Progress, Done}`. Tests: `docket-router/tests/watching.rs`, `intentd/tests/perform_watch.rs`.
4. **`Session.Resolve` answers the label** (ask 147): `IntentsReply::Resolved(Resolved { text, label })`
   (`Session.Display` still answers `Text`); the D-Bus out argument stays one string (now the JSON of
   `Resolved`, so `org.quire.Intents1.xml` is unchanged). readerd classes a read by the handle's own label
   (`intentd/tests/reader_daemon.rs`: mail, not `Prompt`); `resolved_label` is gone.
5. **A restart reads more Spaces than `companiond.toml`'s**: `RecallAsk::Spaces` / `RecallView::Spaces`
   (any session may ask): the router answers the open and paused Spaces memory lists, the Spaces it holds a session
   or a task in, and the desktop; `restore` takes the union with the configured list. Memory's `Spaces` read is
   the shell's today (`almanac-service::auth::allowed`), so the router's own Spaces are the answer until
   almanac lets `Caller::Router` ask it. Ask (almanac): in `allowed`, move `R::Spaces` from the
   `Caller::ShellUi`-only group to `R::Spaces => yes_if(router_or_shell)`.
6. **The `Proceed` wait is a setting**: `AgentConfig.confirm_proceed` (`agent.confirm.proceed_ms`, 10 000, serde
   default so an old `intentd.toml` still reads; shipped in `dist/intentd.toml`). `FixedClock` is a virtual clock
   now (`FixedClock::at`, `advance`, `asked`, `next_ask`; `after(Millis(0))` still completes at once), and
   `a_watcher_that_never_proceeds_is_refused_unasked_after_the_configured_time` runs on it: the router asks for
   the configured time, 2 499 ms more does nothing, the last millisecond expires the step.
7. **Ask 42, cross-Space targets**: see Router item 4.
8. **Asks 43 and 138**: see Router item 3; nothing is asked of stoker.
9. **`companion.task.message` goes through the router**: companiond no longer carries it out (`message_task`,
   `send_to_target` are gone); it is a `Run.Perform` like any call (gated, budgeted, audited) and the hosted provider
   sends the message to the task record's own agent (a worker or a run, not always a worker). Only `companion.task.start`
   is still carried out in companiond, because the worker's loop runs there.
10. **Ask 105**: the chain was already there (`AppFault::Unavailable` -> `CallRefusal::AppUnavailable` -> exit 6);
    what was missing was a test of it end to end: `Answering::Absent` in the fakes, a router test and
    `quire-do`'s `an_app_that_is_not_installed_is_exit_6_not_5`.

## The f4-e2e acceptance

Branch `f4-e2e`. `crates/docket-accept` and `dev/accept/` (README: what is real, what is scripted, what mailo's
provider must match). The real intentd, companiond, readerd and memoryd run as processes on a private bus with a
scripted `Inference1`, a `Confirm1` server and a mail provider; SPEC section 5 flows (a) and (c) pass in the gate.
Tests: flow (a) Allow through undo and the audit in memoryd, flow (a) refused (nothing sent, nothing journaled),
flow (c) (the injected body never reaches the planner; the reader saw it fenced, with no tools, in class Mail;
`Session.Display` gives the summary to the screen; the Outbound send to `x@evil.example` asks, once only, the
recipient `Quoted{from: Mail}`, taint `ReadUntrusted(Mail)`), and one ignored (finding 1).

1. **The answer never shows the sheet or a plan card.** FIXED (f4-docket-2). companiond performs through
   `Intents::perform_watched` (`companiond/src/drive.rs`) and `companiond/src/plan.rs` keeps the card: one
   `PlanStepWire` per call (`Pending` until `Dispatched`, `Running`, then `Done`/`Failed`); the answer goes
   Thinking, Streaming with `AnswerBody::Plan`, `NeedsYou(Confirm(id))` while the sheet is up, Streaming again,
   Done, with an `Updated` signal at each. The card is the body while the phase is Thinking, Streaming or
   `NeedsYou(Confirm)`; a finished answer shows its words. Test `flow_a_the_answer_shows_the_sheet_while_it_waits`
   (un-ignored) and unit tests in `plan.rs` and `task.rs`.
2. **Records that name no Space go to `desktop`** (`intentd/src/audit.rs`). `desktop` stays the default Space
   (SPEC P6). When memoryd refuses a Space's records, intentd now says so once per Space on stderr (`tell_refused`)
   and counts every lost record in `Control.State` (`KillSwitch::audit_lost`, additive, `serde(default)`). The run
   still registers `desktop` until almanac makes memoryd always know it.
3. **memoryd cannot run as its binary without a Secret Service.** CLOSED (f4-e2e-2, `MEMORYD_KEYS`; sandbox on since f4-e2e-3). Was: `accept-memoryd` is almanac's `main.rs` with
   `MemoryKeys` and no Landlock. Ask (almanac, `memoryd/src/main.rs`): an environment switch such as
   `MEMORYD_KEYS=file:<path>` selecting a file-backed `KeyStore` for a scratch HOME, so the packaged binary itself
   can be started in the jail.
4. **inferd has no replay engine.** CLOSED (f4-e2e-2, porter's replay engine). Was: the run speaks the `Inference1` wire from a fake (`src/inferd.rs`). Ask
   (porter, `inferd`): `[engines.<name>] replay = "<file>"` in `inferd.toml`, an engine host that answers the OpenAI
   compatible chat route from a cassette of (request match, SSE reply) pairs, with the planner, writer and reader
   replies of `dev/accept` as its first cassette; then the acceptance can run the real inferd binary.
5. Observed, no action: entity ids in a step's value are shown to the planner plain even when the result is
   labelled third-party (only the words of a title are held back); `Session.Resolve` now carries the label, so the
   reader's session is opened for the class of the mail it reads (verified: class `Mail`).

### f4-e2e-2: real inferd and memoryd binaries

Branch `f4-e2e-2`. The acceptance now runs porter's packaged `inferd` (replay engine, cassettes in
`dev/accept/cassettes`) and almanac's packaged `memoryd` (`test-keys`, sandbox off) as processes; the library
memoryd main and the scripted `Inference1` are removed. `crates/docket-accept/build.rs` builds both from their own
workspaces (`$CARGO`, `--locked`, cargo variables scrubbed) into `<target>/accept-siblings/`, so the gate's
nextest archive finds them inside the jail's bound target dir; the paths are baked in as `ACCEPT_INFERD` and
`ACCEPT_MEMORYD`. All five tests pass unchanged in intent. A mutated cassette (a `lacks` the planner's view does
break) makes flow (c) fail, so the cassette match is checked.

- The data class of the reader's session was not provable (inferd's `AuditEntry` had no class): CLOSED in f4-e2e-3.
- memoryd's Landlock sandbox blocked caller identity (reading `/proc/<pid>/exe`); the run turned it off: CLOSED in f4-e2e-3.
- MAP seams 36 (inferd to planner), 37 (inferd to memory) and 38 (router to memory) move to T: they run
  with real binaries on both sides.

### f4-e2e-3: callers by cgroup, sandbox on, class asserted

Branch `f4-e2e-3`. Since porter W2c inferd and memoryd name callers by `/proc/<pid>/cgroup` only, which a jailed
test process cannot satisfy. Both daemons are built with their test-only `test-proc-root` feature (memoryd also
`test-keys`) and run with `INFERD_PROC_ROOT` / `MEMORYD_PROC_ROOT` = `<scratch>/proc`. `World` writes
`<scratch>/proc/<pid>/cgroup` for each daemon it spawns (`.../app.slice/<name>.service`) and for the test
process as sill (`sill.service`). memoryd's sandbox is now ON (no `MEMORYD_SANDBOX=off`); its callers file
(`memory-callers.toml`) has rows for intentd, sill, companiond and readerd; `inferd.toml` names memoryd, intentd,
companiond and readerd by unit. The reader-class assertion is back (flow c: the reader's one audit entry is class
`Mail`). `ACCEPT_RECORD=1` makes the replay engine record request bodies, printed when the world drops.
- Mailo's real provider mode was not built: see the mailo ask below.
- Ask (mailo session), to run `mailo intents` as the provider in the gate: (1) a headless way to seed a store with
  an account and mail without network (today: `mailo watch` syncing from `scripts/live-imapd.py`, a Twisted IMAP
  server, so the account must be configured and synced before `mailo intents` has threads; a fixture store
  builder, or `mailo intents --fixture <dir>`, would do); (2) `mail-app` builds only with quire and pdfrum git
  dependencies (network at build time) and the GUI stack (a ~900 MB debug build): a `--no-default-features`
  headless `mailo-intents` binary (or package) with no `ds`/blitz deps; (3) a caller check that accepts the
  fake-proc-root router (it asks for the owner of `org.quire.Intents1`, which intentd is, so this may need nothing).
  Mailo's manifest also differs from the stand-in's (`dist/intents/org.quire.Mail.toml`), so flows (a) and (c)
  need their own cassettes for it.

## Upstream asks

Nothing below was edited in the other repos. Asks 1 to 4 of the freeze are cleared: almanac's
`RecentEntry` carries `body` under `BodyMode::Json` (companiond reads it through `RecentSource`),
almanac-client's `almanac-service` is behind `in_process`, quire's `ds-intents` has the voice
marks and re-exports `Tally` (docket-ds no longer reaches `ds-core`), and quire's
`docs/workspace-deps.toml` has `pipewire = "0.10"`.

1. **porter `prov` fills** are done (`Label::{trusted_user, untrusted, join}`); docket's tests and
   fakes use them. Labels with an app source and no private scope (`Label` with
   `Source::App`, a model's text) still have no constructor and stay literals.
2. **stoker `model-provider::Shape`** is filled: the reader renders its reply schema through it
   (`ValueSchema::json_schema`) and `tool_schema` routes its text, integer and choice leaves through
   it. Still asked of stoker: an entity-or-handle form (ask 43), so entities and handles can leave
   the hand-written part of `tool_schema`. The reviewer's and the policy writer's schemas are still
   hand-written `serde_json` (ask 14): their records are not `ValueSchema`s.
3. **cua `cua-bus`** exists: `companiond::recover` reads a run's records from `Recent` as
   `ReplayWhat::Run` by the words of `CuaRecord`'s serde form and names no cua type; the strings
   it reads (`cua.run.started`, `cua.asked`, `cua.confirmed`, `cua.taken_over`, `cua.handed_back`,
   `cua.run.finished`, and the `run`, `space` and `outcome.kind` fields) are pinned by a test.
4. voice.md section 3.4 (porter's speech items) is all present in `porter-infer`
   (`TranscribeBegin`, `SpeakRequest`, `AudioFrame`, `HeardDelta`, `ClientFrame::{Audio,
   EndOfAudio}`, `Readiness`): nothing is asked of porter for voice.
5. **porter `DataClass`:** a class for the person's prompts (item 5 above).
6. **stoker `Shape::to_json_schema`** (item 6 above) also blocks `InferReviewer`.

## Spec conflicts resolved

- **`TaskId` home**: porter `prov` (as the brief says), not `docket-core`.
- **`Ruling` and its words live in `docket-core`**, not `policy-point` (SPEC section 2) or
  `action-review`: `tighten` consumes a `Ruling` and may not depend on `policy-point`, and the
  refusals and audit records name `AskReason`, `Stage`, `Impact`, `ReasonCode`, `BreakerTrip`,
  `Strictness`, `ReviewError`. The logic stays where the spec put it.
- **`PolicyWriter`, `Reader`, `Confirmer`** are traits of `docket-core` (SPEC section 3.5 and
  addendum A1), so `ReviewError` is `docket-core`'s too.
- **Roster types live in `docket-core`**, not `companion-wire` (G12): `PlannerView` holds them and
  `companion-wire` depends on `docket-core`. `Companion1` exposes them as `Roster()`.
- **`planner_view` and the `HandleTable` live in `docket-router`**, not `docket-core` (SPEC 3.8):
  the table is router state, and `docket-core` keeps only the shapes.
- **Episodes are almanac's** (`Episode`, `Skeleton`), not docket-owned as research C.2 first drew
  them; `docket-core` depends on `almanac-core` to build the skeleton (`skeleton_of`, `close`) and
  the router records it at task end. The idle pass writes the narrative through `Session.Note`.
- **Intents1 members**: `Session.Open` takes a `SessionOpen` (Space, agent, parent) not a Space;
  `Session.Resolve` takes the session too (a handle table is per session); new `.Session.Note`,
  `.Session.Recall` (the router applies labels to what memory returns, so a planner never reads
  untrusted recall) and a new `.Message` interface (`Send`, `Inbox`, content-free `Arrived`);
  `options a{sv}` (the reserved `traceparent`) on every call that starts work.
- **`Companion1`** gains `Roster()`, `Front()` and `RosterChanged()` beside the SPEC list; it still
  has no presence, undo, activity or pause member (asserted by a test).
- **Caller roles are a set**: `CallerId.roles`, because sill is the launcher, the confirm server
  and the control centre; a call acts in the first of its roles that may make it (`acting_role`).
- **`tighten` takes the planned stages** (`tighten(ruling, planned, verdicts)`), so a partial
  verdict list can never run a call; **`note(breaker, decision, limits)`** takes the limits, and
  `DenialMark` carries an `ArgDigest` so probing (same goal, different arguments) and an exact
  repeat (`repeated`) are decidable.
- **A Quick flag cannot be overruled.** `escalate` adds the Deliberate stage after a low-impact
  Quick flag, but `tighten` requires every planned stage to allow, so the flag still asks (see the
  open questions).
- **Name collisions** (SPEC section 2): `Verdict` and `Decision` of porter kept; the reviewer's is
  `ReviewVerdict`; the policy's is `Ruling`; `Reveal<T>` for the plain-or-handle value; `ParamNeed`;
  `RepeatState`; `DenyCode` to the agent. `Envelope` exists in `docket-core` (Intents1) and in
  `voice-wire` (Voice1), in different crates; the assembler's budget is `AssemblerBudget`
  (`companion.budget.*`), since `Budget` is the session's.
- **Rig item 10**: `ValueSchema` renders through stoker's `Shape` (`ValueSchema::shape`), inferd
  owns repair and retry; the reader, reviewer and policy writer name only the shape. Every
  `agent-loop` and `voice-loop` step input is `Serialize + Eq`.
- **Voice**: `VoiceIntent`, `SummonOrigin`, `SummonAnswer` and `SummonSerial` are `docket-core`'s
  (voice.md section 3.5); `TurnVia` is on `TurnIn` and `UserTurn` and grants nothing.

## Decisions beyond the spec

- `docket-core` depends on `almanac-core` and `model-provider`; `policy-point`, `action-review`,
  `docket-router`, `companion-wire`, `agent-loop` take `porter-core` or `almanac-core` directly;
  `docket-dbus` takes `porter-dbus` (for `Details` and `OPTION_TRACEPARENT`, not redefined);
  `voice-wire` takes `porter-infer` (`Readiness`, `ServedBy`, `ModelError`, `InferRefusal`);
  `readerd` takes `docket-dbus`. The edge table in `ARCHITECTURE.md` is the exact list.
- The router is generic over `Seams`, one bundle of nine seams, instead of nine type parameters;
  `MemoryLink` is the router's read of memory (`Caller::Router`), `GrantStore` and `EventSink` as
  specified.
- A message needs a router stamp (id, sender, label, time), so a sender hands over a
  `MessageDraft`; its text parts are typed, or a `Handle` the router resolves and whose label folds
  in. It is the only message-shaped type docket adds, and it is not a result type.
- Roster lines for another Space are `RosterDetail::PresenceOnly`: the goal, last step and told
  line are absent from the type, not hidden by a flag (`Roster::seen_from`).
- `companion.task.start` and `companion.task.message` are `Effect::Read`: starting a child session
  or sending a message grants nothing and every call the receiver makes is gated on its own.
- Completion notes are built from typed facts (`CompletionNote`), never from what a worker said;
  the orb waits only for a failure or a report that asked the person something.
- Side conversations end after two idle minutes or when the row closes (`IdleRules.side_close`);
  each turn re-derives the subagent's policy (`SideEffect::Rederive`); the idle pass starts after
  30 s of quiet, yields to anything interactive, and a failed narrative waits for the person to
  come and go (no retry loop). Both numbers are settings.
- The restart rebuild shows a rebuilt line with presence only (goal text is not in the digest)
  unless the person's last direct words are known.
- The eval corpus adds `Expect::{MessageDelivered, BreakerTrips}` and `ArgFrom`/`ScriptedStep` to
  the addendum's types, and puts the cross-Space cases in `eval/cross-space/` using the existing
  corpus names.
- The units in `dist/` are skeletons with sandbox lines (`RestrictAddressFamilies=AF_UNIX`,
  `PrivateNetwork=yes`, `ProtectSystem=strict`, `MemoryDenyWriteExecute=yes`, a syscall filter, no
  capabilities); a test checks every unit and every activation file.

## Open questions for the user

1. A Quick flag on a low-impact call asks even if the Deliberate stage then allows (literal reading
   of "run only from `AllowFinal`, or `AllowJudged` with every planned stage allowing"). If the
   larger model should adjudicate the small one's flags (the shape of the products in the research),
   `tighten` changes in one line and the cascade tests change with it. Your call.
2. `companion.task.message` is a read. If a message across Spaces should be at least an undoable
   write (and so reviewed when tainted), say so; delivery itself is already gated by the receiver.
3. `docket-core` linking `almanac-core` means every app that uses docket's client links almanac's
   pure vocabulary. The alternative is a docket-only mirror of the episode and recall shapes.

## inferd link (ask 81)

Closed by F3 (ask 95): the one constructor is `docket_dbus::inferd_transport` (feature `inferd`);
intentd's `inferd_transport`, `PlannerModel::on_bus` and `ReaderService::on_bus` call it.

`ConfirmReceipt` literals gained `covers` (`Confidentiality::Secret`: a confirmation here opens
nothing; docket never calls `declassify`, so no caller handles its `Result`). intentd's
`inferd_transport(connection)`, `InferdModel::on_bus`, `InferdWriter::on_bus`, companiond's
`PlannerModel::on_bus` and readerd's `ReaderService::on_bus` build
`AnyTransport::Dbus(DbusTransport::over(connection))` (porter-client with feature `dbus`); nothing is
called until the first session, so an absent inferd is `Unreachable` at the first `open`. The models' and the
reader's bodies are filled (see "The bus path") and tested over a scripted session (`ScriptedInferd`, in
`intentd/tests/support/inferd.rs`, porter-fake's session with the opens and frames logged). The companiond and
voiced `main`s are still skeletons (exit 2) and the planner's and `serve` bodies are `todo!()`: whoever fills them
builds the connection first and passes it here.

## f4-docket-2: edges and renames

- **`agent.mcp.expose` and ds-settings.** docket's boundary table does not allow quire's `ds-settings` (the `agent`
  key rows are `docket-core::SETTING_ROWS`, the keys are sill's). actions-mcp therefore reads
  `$XDG_CONFIG_HOME/docket/settings.toml` (`[agent.mcp] expose = "on"`) with a small lenient reader
  (`actions-mcp/src/settings.rs`) and docket ships a static design/22 schema, `dist/settings/docket.settings.toml`
  (`actions-mcp --write-schema <dir>` writes it). Question for quire: should docket depend on `ds-settings` (derive
  and `write_schema`) for the whole `agent` domain, and is `docket/settings.toml` the file design/22 wants? The old
  `access` key of `actions-mcp.toml` is gone (a file that still has it is refused). `McpAccess` is now `McpExpose`.
- **Rename `mail.thread.find` to `mail.thread.search`** in docket-accept (fixture, provider, planner scripts, tests,
  README) to match mailo. porter's `inferd` cassette `docket-flow-a.jsonl` still says `mail.thread.find`.
- **First-use consent** is tested (`first_use_of_mail_in_a_space_asks_once_and_the_second_call_does_not`): the scripted
  sheet gained `Verdict::AllowAlways`.

## f4-docket-3: cards and opening a Hit

1. **`Companion1.Answer.Act(action s) -> o`** is served (`companiond/src/act.rs`, `serve.rs`). `action` is a
   `CardActionId` (the bare id or its JSON string). Only the connection that owns the shell's bus name
   (`org.quire.Shell`) may call it, the same guard as `Open`/`Ask`/`Close`/`Told` (`AccessDenied` for anyone else; an
   id the answer does not offer is `InvalidArgs`). It finds the card in the answer's structured body
   (`AnswerBody::DraftReply` / `ProposedEvent` `actions`, held as `TaskRuntime::proposal`), starts its `CallRequest` as
   a watched `Run.Perform` through `Intents::perform_watched` as the companion role in the answer's session and window,
   and returns at once with the request's object path (`/org/quire/Intents1/request/<n>`; the router in process has no
   object and the path is only the task's call number). The call is a call like any other: gated, confirmed (the
   shell's own `Confirm1` sheet), journalled, audited. The answer follows it with `Updated` (`View` is always current):
   the body becomes `AnswerBody::Plan` with one step per pressed card, `Pending`, `Running` once dispatched,
   `Done { undo }` (the journal row) or `Failed(CallRefusal)`; the phase is `Streaming`, `NeedsYou(Confirm(id))` while
   the sheet is up, then `Done` (also when the step failed: the task is fine, the card's call was not). More than one
   card of the proposal may be pressed; the steps accumulate until the next `Ask`, which resets them. Nothing in the
   loop makes a proposal body yet: `Companiond::propose(task, body)` is the seam for whoever does (the planner's
   structured output). Caveat: a task that has finished has closed its router session, and the router refuses a
   closed session's calls, so a card pressed on a finished task fails (`Failed`); a proposal is only actionable while
   its task stays open (decide when the planner starts making drafts: keep the task open until its cards are pressed
   or dismissed). `docket-client` gained `PerformWatch::request()` and a defaulted `Steering::request` (additive: no struct literal breaks).
   Tests: `companiond/tests/act.rs` (private bus), unit tests in `act.rs`.

2. **Opening a Hit: the convention, validated.** SPEC §3.5 has the launcher perform `Activation::Intent(CallRequest)`
   on a row, with the row's actions taken from the manifest's `ActionDecl`s whose target fits the row's kind, so a Hit
   needs no field of its own and `Follow::Open(EntityId)` stays what it is, an app asking its host to show a thing it
   just made. The contract:

   - Every entity kind an app indexes declares an action named exactly `<kind>.open` (`mail.thread` ->
     `mail.thread.open`; `docket_core::open_name(&kind)`; `Manifest::open_of(&kind)` finds it). Shape: `on = One(<kind>)`,
     `effect = read`, `undo = not_undoable`, no required parameter. `reach = hidden` unless the app wants agents to open
     windows too (it stays callable by the person). `result = nothing`; the app shows the thing in its own window (focus
     it if already open) and answers `Outcome` with `follow = Nothing`. Not indexed kinds that the app's live `Search`
     returns as hits follow the same convention, but `validate` cannot know them: it checks only indexed kinds
     (`ManifestError::OpenMissing(kind)`) and the shape of any `<kind>.open` of a kind the manifest declares
     (`ManifestError::BadOpen(action)`), so mailo's live-searched `mail.thread` (not_indexed) is not broken today; it must
     add `mail.thread.open` (and `mail.draft.open`) before the launcher offers Enter on a mail hit.
   - sill: on Enter for a Things row of `Hit { entity }`, send `Run.Perform` as the launcher with
     `CallRequest { action: ActionRef { app: entity.id.app, name: open_name(&entity.id.kind) }, target:
     TargetValue::Entities(vec![entity.id]), args: Args::new(), origin: Origin::Launcher }`, no session. Effect Read by
     the person: no sheet. If the app declares no `<kind>.open` (`Registry` has the manifests; `Manifest::open_of`),
     the row has no Enter action: show it dimmed or fall back to the row's other actions. A later
     `Outcome.follow = Follow::Open(id)` from any action is performed the same way by the host.
   - Fixture: `docket-fake` mail declares `mail.thread.open` and records it (`FakeMail::opened()`). Tests:
     `docket-core/tests/manifests.rs` (`an_indexed_kind_declares_its_open_action_and_every_open_has_the_one_shape`),
     `docket-router/tests/index.rs` (`a_hit_is_opened_by_its_kinds_open_action_performed_as_the_launcher`).

3. **The other f4-docket-2 items** are not behaviour over fakes in docket's own paths: the `ds-settings` question is
   quire's, the `mail.thread.find` rename is in docket-accept (lane f4-e2e-2's files), the `inferd` cassette is porter's;
   first-use consent was already tested. Nothing further closed.

## f4-docket-ident: cgroup identity and the activation token

1. **Caller identity moved off `/proc/<pid>/exe` onto the cgroup** (`intentd/src/peer.rs`). The bus still names the
   pid; `porter_dbus::ProcCallers::caller_of_pid` reads only `/proc/<pid>/cgroup` (no ptrace check, works in a
   Landlock domain). Order, unchanged for names: a connection that owns application-shaped well-known names is
   those names, with `intentd.toml`'s roles (confirm = owner of `org.quire.Confirm1` through the `confirm` row).
   A connection that owns no name is identified by its cgroup alone:
   - `app-[<launcher>-]<id>-<n>.scope` or `app-flatpak-<id>-<n>.scope`: that app, with the roles `intentd.toml`
     lists under `<id>` (none for almost every app: a plain app). This is also how a user whose terminal runs in
     an app scope (konsole in `app-org.kde.konsole-<n>.scope`) opts in: list `org.kde.konsole` under `cli`.
   - **a terminal child: `vte-spawn-*.scope`, `tmux-spawn-*.scope`, a login `session-*.scope`: `org.quire.Do`, the
     cli role.** porter-dbus returns nobody for these (they are no app scope), so the cli role has this rule of its
     own. **Security decision for the orchestrator to review:** the rule is same uid (checked as before) + a bus
     connection owning no name + a cgroup leaf in that short list. It is NOT "any unidentified process": a service
     unit (`foo.service`), `run-*.scope`, an unreadable or malformed cgroup are refused (`NotAllowed`). It is
     weaker than the old rule in one way: any process in a terminal scope is the cli, not only a binary named
     `quire-do` (the old exe name was as forgeable by a copy of any binary); and stronger in another: a binary
     named `quire-do` outside a terminal scope (a service, a cron job's scope) is no longer the cli. The cli role
     still asks for everything that is not a read, so it stays advisory on a desktop where every process runs as
     the person. No callers file is read: intentd has no unit rows (porter's `CallerTable` is passed empty); a
     daemon that is a service owns a well-known name, which already names it.
   - Tests: `peer/tests.rs` (a table of names x cgroup facts; the cgroup leaves over a fixture proc tree),
     `tests/identity.rs` (over a private bus with a fake proc root: sill's token goes through, a nameless
     connection is cli / plain app / nobody by its leaf).
2. **`INTENTD_PROC_ROOT` / feature `test-proc-root`** (`intentd/src/procroot.rs`, the same three guards as memoryd and
   inferd): off by default; `scripts/check-boundary.sh` fails if `cargo tree -p intentd` shows a `test-` feature in a
   default build; with the feature the daemon prints one stderr line naming the root
   (`TEST BUILD: reading callers from the proc root <dir>, not /proc`); without it a set variable is ignored with one
   stderr line (`INTENTD_PROC_ROOT is set but this build has no test-proc-root feature ...`). Tests: pure
   `proc_root_choice` table, and the real binary on a private bus for whichever build is under test
   (`tests/binary.rs`). docket-accept needed no change: its test processes own bus names, so intentd never reads
   their cgroups; `Peers::with_proc_root` and `serve_on_with` take a `ProcRoot` for tests that do.
3. **Activation token.** `docket_core::Invocation` is unchanged (consumers' struct literals keep compiling). The
   token rides beside it: `ActivatedInvocation { #[serde(flatten)] invocation, activation: Option<ActivationToken> }`
   is what `IntentProvider1.Perform` carries; JSON is the invocation's with a top-level `"activation": "<token>"`,
   absent when there is none, so a provider that reads an `Invocation` is unaffected (tested). `ActivationToken` is a
   newtype over `String` whose `Debug` is `ActivationToken(..)`; audit records are built from the call, never from
   the delivered invocation. `IntentsRequest::Perform` gained the optional field; over D-Bus it is the string
   option `activation` (`docket_dbus::OPTION_ACTIVATION`) in `Run.Perform`'s `options` (`a{sv}`), next to `watch`.
   The router keeps it only when the acting role is `launcher` and drops it for companion, cli, mcp, field, cua and
   plain apps; it reaches the app for the first call of a chain only (a `Follow::Next` step and a `DryRun` carry
   none). Additive seams, all defaulted: `AppLink::perform_activated` (default: perform without the token;
   `DbusLink` and `HostedLink` deliver it), `IntentProvider::perform_activated` (default: ignore the token, call
   `perform`). `docket-client`: `Intents::perform_activated` and `perform_watched_activated`.
   Tests: `docket-router/tests/activation.rs` (launcher gets it through; cli, field, companion, mcp dropped; wire
   form), `intentd/tests/identity.rs` (launcher token crosses the real bus; the cli's is dropped),
   `intentd/tests/link.rs` (the token reaches a real provider through `DbusLink`).
4. **The shell's scope row follows sill-session.** In a real login sill-session will start sill as
   `sill-shell.scope` (`systemd-run --user --scope --unit=sill-shell`). porter-dbus supports an exact named
   non-`app-` scope unit row as of porter 4c2e696, but sill-session's scope change is not on sill master yet, so
   intentd keeps the bus-name rule for the shell's roles (owner of `org.quire.Shell` is launcher/control, owner of
   `org.quire.Confirm1` is confirm) and adds no scope rule and no callers file. When sill-session lands, intentd
   gains a one-row callers table (`org.quire.Shell` = `sill-shell.scope`) and docket-accept's fake proc root puts
   the shell at `0::/user.slice/user-1000.slice/user@1000.service/app.slice/sill-shell.scope`.

## f4-docket-skills: skills (agent-spec skills.md)

What landed: the skills format, discovery, validation and the three places they act.

- **Where the code is.** A new small crate `docket-skills` (not docket-core: it parses `skill.toml`, so it
  needs `toml`, and docket-core stays serde-only; the boundary script now has a `docket-skills` row). It holds the
  parser (`parse_facts`, `parse_doc`, `parse`), the typed `SkillFault`, `discover(&Roots)`, `Library::check`
  against the registered manifests, `preselect`, `Loaded` (the cap of three) and `uses_first`. docket-core gained
  only the planner-facing shapes (`SkillId`, `SkillVersion`, `SkillCard`, `SkillText`) and two `PlannerView`
  fields, `skills` and `skill_texts` (`agent-loop::Sources` has the same two).
- **Only installed files.** `discover` takes `Roots` (directory paths) and nothing else; there is no way to give
  it text. Roots are built from the environment only in a daemon's or quire-do's `main`
  (`Roots::from_env`; intentd uses its own `data_dirs`). `$XDG_DATA_HOME/quire/skills/<id>/` wins over
  `$XDG_DATA_DIRS`; earlier data dirs win over later; a directory that fails does not shadow another of its id.
- **Trust.** Shipped text is `SkillText` with integrity Trusted and `Source::App(owner)`; the person's own is
  `Source::User`. The prompt introduces each body as installed text for the planner that grants nothing.
- **Load.** `companion.skill.load { id }` is in `manifests/org.quire.Companion.toml` (Read, no undo, instant) and
  served by the router's `companion_perform` (`docket-router/src/skills.rs`), so intentd's hosted provider and the
  fake world run the same code. A skill is loadable only if every action it uses is registered and not Hidden;
  the cap of three per session is `SessionRecord::skill_loads`; reloading one already loaded is free. intentd
  installs the skills at start (`Router::install_skills`) and logs rejected and hidden ones on stderr. They are
  not rescanned while it runs (the manifests are); a new skill needs an intentd and companiond restart.
- **Planner.** companiond keeps a task's loaded ids (`TaskRuntime::loaded`), shows loaded bodies and the preselected
  ones (always first, then by id, at most two) every turn, and puts the loaded skills' actions first in the action
  list. The tool order of the request now follows `view.actions` (it was the catalogue's order filtered by the
  budget; the same thing until a skill reorders). `SessionRecord::SkillLoaded { task, id, version }` is written
  through `Session.Note` (slug `skill_loaded`); the restart rebuild ignores it.
- **Not done.** `evals/*.toml` cases in a skill directory are not run (skills.md section 6): no case format for
  them exists yet; `--check-skills` checks everything else in that section. Skill text is budgeted: a planner view
  carries at most `BODY_BUDGET_BYTES` (12 KiB) of bodies, loaded and preselected together. Loaded bodies are
  committed first (a load that would pass the budget is refused with `LoadRefusal::OverBudget`, which the planner
  reads as a refused Read; nothing is truncated and nothing is audited as loaded; a reload is free). Preselection
  gets what is left, takes `always` first then `when` matches, and stops at the first body that does not fit. The
  shipped `desktop-basics` is under 1 KiB; owners should keep bodies far below the cap.
- **`docket-eval --check-skills <dir>... [--manifests <dir>]...`** walks each directory for `skill.toml` files and for
  manifests (the `--check-app` rules, which include `dist/intents/*.toml`), also reads `dist/intents/*.toml` under
  each directory and the `intents` directory beside a directory named `skills` (so naming `~/sill/dist/skills`
  finds `~/sill/dist/intents`), adds every `*.toml` of each `--manifests` directory, and adds the two built-in
  manifests. A manifest reached twice is read once. Pass every repo whose actions the skills use. A missing
  action fails; an action Hidden from the companion is a note.
- **`quire-do skills`** lists the installed skills from `$XDG_DATA_*`, each marked offered, hidden (missing action,
  with which) or not offered (Hidden action), plus directories that did not load. It needs intentd for the manifests.
- **voiced.service** lost its `[Install]` section: voiced was a skeleton that exits 2, so `systemctl --user enable`
  must not start it at login in beta. `serve` is filled now (v-voiced), but the section stays out until the owner
  says the daemon may start with the session.

## f4-settings: the settings schema and its reader

The Settings app (detent) builds its Intelligence page from the schema each daemon ships (design/22
section 9.2). docket's covers every `agent.*` key of section 3.27, and the daemons read what it offers.

1. **`dist/settings/docket.settings.toml` has 18 rows**, one per key of design/22 section 3.27, none
   `agent = "settable"` (`agent.*` is never agent-settable). **On the Intelligence page** (section 5, Privacy):
   `agent.strictness` (segmented), `agent.mcp.expose` (toggle), `agent.undo.keep_h` (1..=168 hours).
   **Advanced:** `agent.review.{quick,deliberate,second}_ms`, `agent.budget.{calls,writes,outbound,destructive,
   per_minute,fan_out,chain,wall_s,reviews,denials_in_a_row,spend_microusd}`, `agent.task_policy.max_min`. The
   design gives no range for the budget rows ("per session"); the ranges in the schema are ours (calls
   1..=10000, writes 0..=10000, outbound and destructive 0..=1000, per_minute 1..=600, fan_out 1..=10000, chain
   1..=16, wall_s 60..=86400, reviews 1..=10000, denials_in_a_row 1..=20, spend 0..=1e9 micro-USD): the person
   may lower a budget and the top is generous. Design rows 3.27 names for the review times and `task_policy`,
   `undo`, `mcp` are the design's.
2. **The reader is `docket-settings`** (new crate; `read(text, base)`, `Locator`). The file is
   `$XDG_CONFIG_HOME/docket/settings.toml` (then each of `$XDG_CONFIG_DIRS`; the first file that reads wins
   whole). A value that is the wrong type, out of range or not a word of its key falls back **to the base
   value of that key** (intentd.toml's `[agent]`, itself the shipped defaults) and is returned as a
   `Fallback`; the daemon logs one line per fallback and per unknown key (`intentd: settings: agent.budget.
   calls: outside 1..=10000; using the previous value`). A file that is not TOML keeps every base value. A key
   the file stops setting is its base again (each read is over the same base, not over the previous read).
   `denials_in_a_row` is `AgentConfig.breaker.consecutive`; the rest of the breaker (`recent`, `window`,
   `probing`) is not a key of 3.27 and stays in intentd.toml.
3. **intentd follows the file live** (`settings_watch.rs`): a `notify` watch on `$XDG_CONFIG_HOME/docket`
   (created if missing), 30 ms debounce, the whole file read again, then `Router::apply_settings`. The router
   reads `agent_config()` afresh in every decision (a settings value wins over `Router.config`, which tests
   still set directly). A session already open keeps its ledger; its next charge is measured against the new
   budget. The reviewer cascade's own timeouts are the ceiling (`REVIEW_CEILING`, the top of the ranges), so
   the router's live `agent.review.*` deadline is the one that fires. If the directory cannot be watched the
   settings are read once and one line says so (`SettingsWatch::state` is `Blind`).
4. **actions-mcp reads `agent.mcp.expose` again at every `tools/list` and call** (`McpEdge::with_expose_read`),
   not through a watch: its boundary row forbids `notify`. A change applies to the next request; the edge
   does not push a `tools/list_changed`. Closes when rmcp's list-changed notification is wired (no ask yet).
5. **companiond reads the file once, at start** (`CompaniondConfig::from_env` overlays it on `[agent]`).
   The keys it uses are `agent.budget.calls` (the session call count) only; making it live means its
   `Runtime` holding the config behind the same `apply_settings` shape, in `runtime.rs`, which the
   f4-docket-shown lane owns. Closes when that lane has merged: one `RwLock<AgentConfig>` in `Runtime` and a
   `settings_watch` task in companiond's `daemon.rs`, as in intentd.
6. **Left out of the schema, and why.** Fields of `AgentConfig` with no row in design/22 section 3.27 are not
   settings: `mass_at`, `confirm_expiry`, `confirm_proceed`, the breaker's `recent`/`window`/`probing`, and
   `companion.*` (assembler budgets, idle rules). They stay in `intentd.toml` / `companiond.toml`. docket-core's
   `SETTING_ROWS` still lists them under the older names (`agent.task_policy.max_s`, `agent.budget.wall_s`,
   no `spend_microusd`) and is read by no daemon; it closes when section 3.27 and the table are made one list
   (the new crate's table is the one the schema is held to).
7. **Consumers' lockfiles** (cua, sill) gain `docket-settings` and `notify` when they next update docket:
   `cargo update -p intentd` (cua also takes it through its dev-dependency on intentd).
8. **Tests.** The schema parses with the Settings app's loader (`ds_settings::Schema::from_toml`, run by hand
   from a scratch project: a dev-dependency on ds-settings would unify zbus's executor features across this
   workspace) and is held structurally by `docket-settings` tests; every schema key is read (table test);
   every key has a bad-value fallback case; `intentd/tests/settings_live.rs` changes the file under a running
   router and waits on the watch's own event; `docket-router/tests/live_settings.rs` changes strictness under
   a live router; actions-mcp's edge follows a changed file.

## f4-docket-calleffect: per-call effect (sill G372)

An action's declared effect is a **ceiling**; an action may opt in to a per-call effect, so one action
(sill's `shell.menu.activate`) does not have to ask for every call.

- **Manifest.** `per_call = "classified"` on an `[[actions]]` row (`ActionDecl::per_call: PerCall`, serde
  default `declared`, not written when it is the default; existing files and `..`-less Rust literals other than
  docket's own test fixtures are unaffected, and no `ActionDecl` literal exists in sill, cua, almanac or mailo).
- **Classify.** For an opted-in action called by an agent session, the router first asks the provider
  (`IntentProvider::classify`, `IntentProvider1.Classify(invocation s, options a{sv}) -> s`, the answer is a
  `Result<CallClass, AppRefusal>` in JSON; `CallClass` is `{"kind":"effect","v":"read"}` or
  `{"kind":"delegates","v":{"app":"...","name":"..."}}`). The person's own calls are not gated and not classified.
  The pure rule is `docket_router::classify_step` (table-tested): effect used = `min(answer, ceiling)`; an error,
  a timeout, a provider without `Classify`, a delegate that is unknown or the action itself = the ceiling.
- **Delegates.** The outer call is gated as `Read`; the provider performs the real action by answering a
  `Follow::Next(call)` for exactly that action, which the router runs as a child in the same chain and session
  with its own full gate (one ask for an inner Destructive action, none for an inner Read one). The outer call's
  own effect is `Read` once it delegates, so `Delegates` cannot run a Destructive effect unasked. No follow is
  audited `Delegation::Unused` (nothing beyond Read happens); a follow for another action is gated normally and
  audited `Different`. A provider that calls `Run.Perform` itself instead is not a child and shows as `Unused`.
- **Bound to Perform.** The classification gated on goes to `Perform` as a top-level `"classified"` beside the
  invocation (`ActivatedInvocation::classified`, skipped when absent; `IntentProvider::perform_classified`). A
  provider re-derives and refuses `AppRefusal::ClassificationChanged` on a mismatch, except that `classified`
  equal to the declared ceiling must be accepted. The router then re-gates the same call at the ceiling (a new
  call id, no new Classify) and performs again with `classified` = the ceiling.
- **Audit and policy.** `AuditRecord::Classified { call, action, classification { ceiling, answer, used, sent } }`
  (a second one, answer `failed: changed`, after a retry) and `AuditRecord::Delegation`. `AuditRecord::Call.effect`,
  Cedar's `resource.effect`, the budget cost and the taint rule all see the effect used, so strictness rules apply
  to it unchanged (tests: AskMore asks for a classified undoable write; TrustMore still asks for a Destructive one
  outside a task policy). A dry run (`Run.DryRun` preview) is prepared at the ceiling.
- **Tests.** `docket-router/src/classify.rs` (table), `docket-router/tests/percall.rs` (docket-fake's `FakeMenu`),
  `intentd/tests/percall.rs` (a router over `DbusLink` and a real provider on a private bus).

## f4-demo-install: the dev installer and `quire-do ask`

For the owner's live demo of the companion on a cloud model (runbook: `docs/demo-cloud.md`).

- **`dist/install-dev.sh`** installs the stack for one user from the sibling checkouts (`../porter`,
  `../almanac`, `../stoker`, this one), builds with `cargo build --release --locked`, never runs sudo, and
  records what it wrote in `~/.local/state/quire-dev/install-dev.manifest` for `--uninstall` (a config the person
  edited is kept; directories are removed only if the install made them and they are empty). Tested in a jail
  (`docket-cli/tests/install_dev.rs`: HOME, XDG and PREFIX in a tempdir, fake binaries through
  `INSTALL_DEV_BIN_DIR`, cleared environment).
  - memoryd has no unit in `almanac/dist`: `almanac/dbus/memoryd.service` is its unit, started by D-Bus
    activation (`org.quire.Memory1.service` names it); that is what is installed.
  - Beyond the brief, because the demo does not work without them: porter's `providers/*.toml` (accountd cannot
    `add openrouter` without its provider file) to `$XDG_DATA_HOME/porter/providers`, and stoker's `catalog/*.toml`
    (inferd reads its model entries only from `$XDG_DATA_HOME/stoker/catalog` and `/usr/share/stoker/catalog`;
    without them there is no `cloud/<entry>`).
  - Left out on purpose: `voiced` (skeleton), `syncd`, `cuad`; `docket/manifests` (the Memory and Companion
    manifests are built into intentd, which logs "built into intentd" and skips a file of theirs). `intentd.toml`,
    `companiond.toml` and `actions-mcp.toml` are installed only if absent as asked, but note that each replaces the
    built-in default whole, so a later change to the shipped file does not reach a machine that has a copy.
  - Skills land in `$XDG_DATA_HOME/quire/skills`, which docket-skills reads as the person's own (`Origin::Own`,
    source `User`), not as shipped (`Source::App`). Fine for a dev install; a package installs under `/usr/share`.
  - **Dev-only change to the packaged units** (f4-demo-install-2, after porter e7b8e35): `ProtectHome=yes` becomes
    `read-only` for companiond and readerd when the prefix is under `$HOME`; their units still say `yes`, and with it the
    binary in `~/.local/bin` cannot be executed (and companiond could not read `~/.config/quire` or the skills). The
    porter units need no drop-ins any more (they make their own directories). `--cloud` installs porter's
    `dist/inferd-cloud.conf` as `inferd.service.d/cloud.conf` (network on); opt-in because local engines share inferd's
    sandbox, so an on-device-only setup keeps none. An install made before this change has its `10-dev.conf` drop-ins
    removed on the next run.
  - The one root step (printed, never run): `sudo install -D -m644 <porter>/dist/callers.toml /etc/porter/callers.toml`.
    accountd and syncd read `/etc/porter/callers.toml` with `$XDG_CONFIG_HOME/porter/callers.toml` laid over it (user
    rows win); inferd's own caller table is `[callers]` in `~/.config/quire/inferd.toml` (installed), and memoryd's is
    `memory-callers.toml` (installed in the user config, which layers over `/etc/quire/`), so neither needs root.
- **`quire-do ask [--space <id>] "<text>"`**: `Companion1.Open` in the Space (default `desktop`), `Session.Turn`
  through intentd, `Companion1.Ask`, then the answer object is followed (its current `View`, then each `Updated`)
  until it is no longer Thinking or Streaming. Prints the text, the plan steps, what is waited for and the footer
  (model, where it ran); `--json` or a pipe prints the answer object. `NeedsYou(Confirm)` prints the request and
  exits **8** (new code, `needs_you`) with "answered in the shell": `quire-do` is not `Confirm1`, nothing answers
  it, and the conversation is left open for the sheet; a finished conversation is closed. A handle in the text is
  printed as `#n` (a terminal may not `Session.Display`). The seam is `companion_client::CompanionTransport`
  (`DbusCompanion` over the bus; a new crate rather than a module of `docket-client`, so cua's and sill's lock files, which
  list docket-client's dependencies, do not change); the whole program is `docket_cli::program::main`, which `accept-quire-do` also
  runs. Tests: `docket-cli/tests/ask.rs` (scripted companion over the fake router), `docket-accept/tests/terminal.rs`
  (the real `quire-do` against the real daemons: a read answered, a Forward left waiting for the sheet),
  `companiond/tests/terminal.rs` (the authority, below).
- **Security: the cli role speaks for the person in three calls.** companiond used to accept only the owner of
  `org.quire.Shell`. It now also accepts, for `Open`, `Ask` and `Close` only, a connection that (1) is the same
  user (`GetConnectionCredentials`), (2) is in a terminal's scope (`vte-spawn-*`, `tmux-spawn-*`, a login
  `session-*`: the same `docket_core::is_terminal_scope` intentd derives the cli role from, over the cgroup of the
  pid the bus names), and (3) owns no well-known name (a name makes it that app, as in intentd's `Peers`). `Told`
  (words to a subagent) and the answer object's `Act` (pressing a card) stay the shell's alone; `Cancel` and `View` were
  never restricted. The reasoning: the cli role is the person at a terminal, same uid, which intentd already
  treats as asking for everything that is not a read, with a confirmation for each act; `Open`, `Ask` and `Close`
  hand the companion words and a conversation, and every call the companion then makes still goes through
  intentd's gate in role `companion` with the sheet for the person. What it adds: a process in a terminal (an
  agent running shell commands, say) can now put words in the companion's mouth, which it could already do to
  `quire-do <app> <action>` one call at a time; it cannot answer a confirmation, press a card, or reach a
  subagent. The router's side: `Member::SessionTurn` now permits `Cli` (it was Launcher and Field only), because a
  turn the router did not record is not a turn; such a turn is recorded as the new `TurnSource::Terminal`, not as
  the launcher's, so a later rule can treat it differently (nothing does yet; sill only constructs `TurnSource`,
  so adding the variant breaks no match there). `COMPANIOND_PROC_ROOT` (feature `test-proc-root`, ignored with a
  line on stderr in any other build, and now checked by `check-boundary.sh` beside intentd's) lets the acceptance
  run present a process as a terminal; `serve_on_rooted` is the in-process seam.
- **Cannot work yet, for the demo.** (1) A confirmation cannot be answered without sill's `Confirm1` sheet: a write
  (`memory.propose`) stops at "Waiting for your confirmation" (exit 8); reads and plain chat work. (2) The first use
  of a data class in a Space also asks; there is no way to give the standing grant without the sheet. (3) The
  `desktop` Space may not exist in memoryd on a fresh install (nothing here creates it; sill does): recall and the
  audit trail may fail softly. (4) A handle in an answer prints as `#n`. (5) Mail, calendar, files: not installed
  (mailo has its own installer). (6) `readerd` is installed but a cloud model is only reached for the classes
  whose floor you lowered. (7) accountd stores the key in the Secret Service: KWallet must be unlocked.

## f4-docket-shown: an answer's handles outlive its task

1. **A finished task keeps its router session open until the answer is dismissed.** `finish` no longer calls
   `Session.Close`; `Companion1.Close` does (`Companiond::dismiss`, shared with the eviction path). The count is
   bounded, not timed: `linger::LINGER = 8` finished-but-open sessions. When a ninth task finishes, the oldest
   finished one is closed in finish order and its answer is dropped exactly as `Close` would. The step is pure
   (`linger::finished` and `linger::dismissed`, table tests). Why 8: the same as `REMEMBERED`, the number of finished
   tasks the roster and the recent-episodes section already keep; a person does not look back at more answers than that.
2. **Memory is unchanged.** The router still ends the task and writes its episode at the finish, and the narrative
   and idle job run then. `finish` sends the new `NoteAsk::End` through `Session.Note`: the router ends the task and
   leaves its skeleton exactly as `Session.Close` did, but the session stays open (its handles can still be shown).
   `Session.Close` at dismissal or eviction then finds the task ended and writes none. A worker's final report still ends
   its task first; `End` after it writes nothing (tested: one episode, never two). Nothing is lost if companiond dies
   while sessions linger; only the open sessions leak until the router restarts.
3. **`Session.Display` on a closed session is refused** (`NoSuchSession`, `docket-router/src/reading.rs`). Before,
   the router kept a closed session's handles and still showed them; now a handle shows until its answer is
   dismissed and not after.
4. **How a surface finds an answer's session.** `org.quire.Companion1.Answer` gained a read-only property
   `Session` (`s`, the `SessionId` text), set before `AnswerAdded` is signalled, so every answer object, whoever
   started the task, names its session. `AnswerWire`, `FrontTask` and `AskWire` are unchanged. sill reads
   `Session` from the answer object (the same proxy it reads `View` from), then calls `Intents1.Session.Display`
   with it for each handle; the object, and the session, go away on `AnswerRemoved`.

## f4-docket-defaults: shipped consent for the shell's own data, and `--check-skills` across repos

1. **One shipped default.** `dist/intents/default-grants.json` holds two grants (Interactive and Background):
   caller Companion, owner `org.quire.Shell`, target App, class `app_own`, every Space, `allow`, `always`. It is
   installed with the other `dist/intents` files (the installer on `f4-demo-install` copies that tree whole, so
   it needs no change). intentd reads `quire/intents/default-grants.json` under every data directory at each
   consent lookup (`FileGrants::with_defaults`) and puts it under the person's `grants.json`.
   Why this is safe: the shell is the desktop itself. Its `app_own` data is window, menu, workspace and dock state,
   which the person already sees and drives from the same screen; nothing in it is mail, files, the screen or
   another app's content. The default names the shell and no other app, so a delegate's inner action (an
   app's own `app_own` rows, e.g. `dev.notes`) keeps the normal first-use ask per app, and the person can answer
   Always there.
2. **Layering.** Defaults are read as grants dated the epoch, so porter's `decide` (the newest grant for the exact
   key wins, a denial wins a tie) lets any entry of the person's outrank them. `consent_for` already tries the
   narrowest key first, so a denial for one action or one Space also wins over the broad default.
3. **Revoking.** No new form was needed: a grant already carries `Decision::Deny`. A revoke of a default is the
   person's own entry with the same key and `decision = deny` (`intentd::revoking(default, id, at)` builds it from
   the default); it is written to the person's `grants.json`, never to the shipped file, and survives a restart.
   A later `allow` from the person brings the key back.
4. **A guard on the file.** The person's own data directory is also a data directory, so the loader accepts only
   `Always` allowances over `app_own` and skips (with a line on standard error) anything else, a denial included. A
   file dropped there cannot lend the companion mail, files or the screen, and a damaged file grants nothing.
5. **Strictness.** The default only answers the consent question. docket's table (`policy/default.cedar`): a Read in
   the same Space is final in all three strictnesses, so a shell Read row (Hide, Minimise) never asks, AskMore
   included. An undoable write still asks under AskMore (judged under Default), an Outbound or Destructive act
   asks in every strictness, and a tainted session still asks again before leaning on an Always for a write.
   Tests: `crates/docket-router/tests/shell_defaults.rs`, `crates/intentd/tests/files.rs`.
6. **`--check-skills`** now finds manifests where apps keep them (item above in f4-docket-skills). The line to change
   in agent-spec `skills-format.md`: "`docket-eval --check-skills <dir>... [--manifests <dir>]...`: manifests are
   those under each dir (as `--check-app` finds them, `dist/intents/*.toml` included), the `intents` directory
   beside a `skills` directory, every `*.toml` of each `--manifests` directory, and the built-in two."
7. **docket-fake** `FakeMenu` now treats any `<app>.item.*` action as a menu item (it matched `menu.item.*` only), so
   a test can stand it in for another app by renaming the manifest.

## f4-docket-why: the answer says why

1. **Routing property.** `org.quire.Companion1.Answer` gained a read-only property `Routing` (`s`): JSON of
   `Vec<companion_wire::RouteNote { stage, served, why: Vec<WhyWord>, reached: Option<Reached> }>`, the last model turn
   that announced anything (`[]` before one did). Set before `AnswerAdded`, like `Session`. `AnswerWire`, `FooterWire`
   and `AskWire` are unchanged, so no literal in sill, cua or almanac breaks.
2. **The line.** `companion_wire::footer_line(&[RouteNote]) -> String` prints "Heard by Whisper · Answered by Claude
   Haiku 4.5 via OpenRouter (already loaded)"; sill and detent both call it. Model names are made from the id
   (`model_name`: "claude-haiku-4-5" is "Claude Haiku 4.5"); a catalogue display name would be better when porter has one.
   Done: `RouteNote.name: Option<ModelLabel>` copies porter's `StageNote.name` (the catalogue label, sent on every
   answer's `Answer` note since porter 11651d4), and the line prefers it; the id-made name is the fallback for a model
   with no label or an older inferd. The field is skipped when absent, so an unnamed note's JSON is unchanged.
3. **Folding.** `RouteLog` folds one turn's events: `Why` words collect until a `Stage` (or the end, for a one-model session
   with only `Routed`) closes a note; `Why::Reached` is the door, not a reason. A `Stage`'s own `why` is always sent by
   inferd, so only its door is used: reasons come from `Why` events, which exist only when `ai.auto.show_reason` is on
   (and for an eviction, always).
4. **Declined.** `PlanFault::Declined` (no longer `Copy`): a `Declined` event before `Finished(Refused)` makes the task's
   refusal `Failed("Kimi K2.6 cannot answer: it is not installed on this computer.")` (`declined_text`).
5. Needs porter at f34da56 or later (`Why::Reached`, `Stage`). The workspace paths point at `../porter`.

## v-voiced: spike V-A and the voiced daemon (voice wave, F2)

Closes the voiced rows of the todo table: `voiced::serve` is filled and the binary serves instead of exiting 2.
`todo!()` in voiced, voice-loop and voice-wire: 1 before, 0 after.

### Spike V-A (PipeWire), by hand

- **Builds and licenses.** `pipewire = "0.10"` (0.10.1, with libspa 0.10.1, pipewire-sys and libspa-sys 0.10.1; all
  MIT) builds here against the system libpipewire 1.6.9 (`pipewire-devel`) and needs `libclang` for bindgen
  (`clang` is installed). `cargo deny check licenses` passes with it; the workspace line is in `Cargo.toml` and only
  voiced takes it (`check-boundary.sh` allows `porter-dbus` and `speech-provider` for voiced and still forbids
  `pipewire` everywhere else). No fallback to libpulse was needed.
- **Listing, read-only, run on this machine** (`dev/voice-capture-try.sh list`; it opens no stream): the registry
  shows three `Audio/Sink` nodes (SPDIF, Speaker, Headphones) and two `Audio/Source` nodes on the USB audio device
  (`...HiFi__Line__source`, `...HiFi__Mic__source`). `choose_capture` picked the **Line** source, the first it met:
  see the open question below.
- **Not yet run, owner's by-hand step**: `dev/voice-capture-try.sh capture 5` (opens the real microphone: 16 kHz
  mono S16 through PipeWire's adapter, frame sizes, widest gap between frames, peak level, and that monitor and sink
  nodes are refused) and `dev/voice-capture-try.sh play` (a tone at 24 kHz). Send the output back; the jitter and
  the adapter's frame size are the two numbers this spike was meant to measure. voiced asks for `node.latency =
  512/16000` (32 ms, one `Level` per frame) and treats the stream as dead if it does not reach Streaming in 2 s.
- **Refusal of monitors** is in three places: `kind_of` classifies `Audio/Source` nodes named `*.monitor` and
  `Audio/Source/Virtual` as `Monitor`; `choose_capture` takes `Source` with class `Audio/Source` only; `open_capture`
  refuses any node that is not a `Source` before it connects. A test per layer.
- Unit file: the sandbox is unchanged except `ProtectHome=tmpfs` with read-only binds of `%t/bus`, `%t/pipewire-0`
  and `%h/.config/sill` (the bus and PipeWire sockets would otherwise sit under the `/run/user` that `ProtectHome=yes`
  hides). `MemoryDenyWriteExecute=yes` and `LimitMEMLOCK=0` are kept; if PipeWire's client library needs either
  relaxed, the by-hand run will say (a failed `mlock` is logged, not fatal). Still no `[Install]`.

### What `serve` does

- `voiced::start(connection, config, Seams)` registers `org.quire.Voice1` (root, then per-utterance and per-speech
  objects) and starts the loop; `serve(config, device)` is `start` over the session bus with `DbusTransport`,
  `BusWarm` and `FileUse`, and runs until the bus closes. `Seams` holds the device, the inferd transport, the
  warmer, the consent source and the `/proc` root: the tests serve the whole daemon on a private bus with a scripted
  device and porter-fake's `FakeInferSession`.
- **Roles from the connection.** `peer.rs` takes the well-known names a connection owns (`ListNames` plus
  `GetNameOwner`, the same user by `GetConnectionCredentials`) and, if it owns none, the app scope its cgroup names
  (`porter_dbus::ProcCallers`, `/proc/<pid>/cgroup` only). `shell` is any connection that owns a name listed under
  `shell` in `voiced.toml`; `app` is never derived, it is whoever owns the name a `Route` names (`GetNameOwner`).
- **One loop, two pure machines.** Every command, capture frame, inferd event and playback report is an input to one
  task that runs `utterance_step` and `speech_step` and carries out their effects. Slow things run elsewhere and come
  back as events: the inferd session (`link.rs`), `Prepare`, playback (`playback.rs`) and each PipeWire stream (its
  own thread). The mic is dropped exactly when the machine says `CloseMic`.
- **No clock in the machine.** The 250 ms tail is counted in capture samples (4000), not in wall time, so a test
  feeds frames and nothing sleeps. Buffered audio (engine cold) is the 10 s `PcmBuffer`, flushed in order when the
  session is ready, in frames of at most one second; past ten seconds the oldest goes (voice.md section 4.2).
- **Signals.** `Ended` and `Finished` are unicast (`set_destination`) to the Begin caller and the attached app, and
  the requester; `Ended` carries the end with `Heard` text emptied. `StatusChanged` is broadcast, content-free. The
  event fds carry the text; nothing carries audio. A test asserts a bystander is sent neither.
- **Dictation** ends by itself on 30 s of silence (`EnergyGate` + `endpoint` from speech-vad, `EndpointSilence`,
  the existing machine row); an Ask never does.
- **Bodies.** Voice1 bodies are `voice_wire::Envelope { vocab, body }` JSON in `s`; a foreign vocabulary or bad JSON
  is `org.quire.Voice1.Error.Malformed`. The refusals are `VoiceRefusal`'s error names, one to one. The event fd frame
  is a 4-byte big-endian length then the envelope (porter-core's framing with `VoiceVocab`). `wire::{seal, frame,
  unframe}` are exported for sill's client.
- Earcons: a two-note blip generated in integers (`earcon_samples`), played on its own stream at 24 kHz when
  `earcons = "on"`.

### Interface notes and asks

1. **`CaptureStream: Send + Sync`** (was `Send`) and **`serve<D: AudioDevice + 'static>`**: the loop holds the
   stream across awaits in a spawned task. Every implementor in this repo already meets both.
2. **porter, `Transport::prepare`** (ask): `porter-client`'s `Transport` has `open` but no `prepare`, so `Voice1.Prepare`
   calls `Inference1.Prepare` through `porter_dbus::InferenceProxy` in `BusWarm`. Wanted: `fn prepare(&self, need:
   &Need, class: DataClass, tier: Tier, options: &OpenOptions) -> impl Future<Output = Result<Readiness,
   TransportError>>` on `Transport`, so voiced drops its porter-dbus use for this. Nothing else is asked of porter:
   `Transcribe`, `Speak`, `Audio`, `EndOfAudio`, `Heard`, `Spoken` and `Readiness` are all there, and the fake scripts
   them.
3. **sill, the consent source** (ask): voiced decides `VoiceUse` per call by reading sill's `settings.toml` (the
   unit binds `%h/.config/sill` read-only). It reads the `[voice]` table: `hold_to_talk = "off"` is Off; `consent =
   "given"` (or a `[voice.consent] given = <unix seconds>` table) is On; `"declined"` is Off; anything else, a missing
   file included, is NeedsConsent. sill's settings crate should write exactly that (the encoding of `VoiceConsent`
   was not frozen), or say another and voiced changes `use_of_settings`. If sill would rather push consent, the XML
   needs a shell-only `SetUse` member: say so and it is added with its introspection test.
4. **sill, the shell side of `Begin`** (ask): body is `Envelope<VoiceBegin>`; the reply is the utterance path and an
   fd of length-prefixed `Envelope<VoiceEvent>` frames; the shell must hold a bus name listed under `shell` in
   `voiced.toml` (`org.quire.Shell` in `dist/voiced.toml`). `Release` ends the hold; the daemon keeps the mic 250 ms
   of captured audio longer; the shell cancels with `Cancel` (cause `Shell`: sill has no way to send `Escape` or
   `OtherInput`, which are the causes its hold machine produces: either `Cancel` grows a cause argument, or sill maps
   both to `Shell`; the transcript is discarded either way). Refusals come as errors named `org.quire.Voice1.Error.*`
   (`NeedsConsent` is the cue for the consent sheet; `MicUnavailable` before anything opens when there is no physical
   source). sill must subscribe to `Ended` and `Finished` before calling (unicast, nothing is replayed).
5. **The consent sheet** is sill's alone; voiced only refuses with `NeedsConsent` and never opens the mic first.

### Decisions beyond the spec

- A `Begin` with no physical source is refused `MicUnavailable` before the machine runs; a source that fails to open
  is `Ended(Failed(MicUnavailable | MicDenied))` as the machine says.
- `Cancel` from the attached app is cause `FocusLost`; from the Begin caller `Shell`.
- An old utterance's object is removed from the bus when the next `Begin` happens, so a late `Release` on it is an
  unknown-object error, not a way to touch the new mic. Finished speech objects go when the next `Speak` is made.
- Tier is `Balanced` for both speech needs until the settings name another (`ai.model.speech_in.<tier>`).
- Merged speak requests (one made while another plays) finish together; a second one made while the mic is open or
  speech is stopping replaces the waiting one (depth one, the machine's rule) and the replaced one finishes `Hushed`.

### Open questions for the owner

- `choose_capture` takes the first `Audio/Source`; on this machine that is the **Line** input of the USB card, not
  its Mic. Should voiced follow PipeWire's default source (`default.audio.source` in the metadata object), or a
  `voice.input` setting by node name? Until then the dev script prints what would be captured.
- `voiced.service` needs `[Install]` (or a dbus activation file) when voice goes live: your word.

### Tests added

voiced: 20 in `tests/serve.rs` (who may begin, consent and monitor refusal, denied mic, the full hold with levels,
signals, unicast, audio order and size, cold engine buffering, cancel, supersede, refusals, dictation endpoint,
capture death, prepare, malformed bodies, name taken, nothing written to disk), 7 in `tests/speech.rs` (sentence by
sentence, who may speak, half-duplex queueing, barge-in, hush, stop, synthesis refused), 2 in `tests/attach.rs`
(route and attach, replay, one attachment, who may release, cancel and speak by the attached app); unit tests for
roles, consent text, framing, errors, earcons, the PipeWire node classification and fade. The by-hand
`dev/voice-capture-try.sh` is the only thing that touches a real device.
