# Findings

Open items and standing facts. An entry names the condition that closes it. After fill wave 1,
the docket amendment and the intentd bus fill (F3) there were **22 `todo!()` bodies** in library and
daemon code (21 lines: `MemoryProvider` and `CompanionProvider` share one macro line), listed
below, one row per crate or file; the tests contain none, and no test is `#[ignore]`d. (The freeze
had 37: `agent_step` was filled in fill wave 1; F3 filled `DbusTransport::call`, `docket_client::serve`,
`DbusLink`, `SheetConfirmer`, `FileGrants` and intentd's `serve` and `main`.)

## Stubs behind frozen interfaces

| Where | Count | Closes when |
| --- | --- | --- |
| intentd `record_of` | 1 | fill wave 2: `AuditRecord` to almanac `Record` (typed `Message` and `Episode` bodies, the rest `Area { Docket }` with the things each names for cascade-forget) |
| intentd `InferdModel::{chat, embed}`, `InferdWriter::derive`, `ReaderClient::extract` | 4 | fill wave 2, blocked on porter's `DbusTransport::open` and session fills: sessions through porter-client; `ReplyShape::Json` of the policy record from the person's turns and the catalogue alone; `Reader1.Extract` |
| intentd `MemoryProvider::perform`, `CompanionProvider::perform` | 2 | fill wave 2: `memory.recall/facts/propose/forget` through memoryd as `Caller::Router` (untrusted proposals land pending); `companion.task.start` opens a child session and records `task.started`, `companion.task.message` goes through message delivery |
| readerd `reader_request`, `ReaderService::{extract, extract_in}`, `serve` | 4 | fill wave 2, with stoker's `Shape::to_json_schema`: the fenced data, no tools, the schema as the reply shape, `conforms` on the answer |
| docket-ds `DsContextSource::snapshot` | 1 | fill wave 2 (quire apps): `ContextModel` to `Here`, `Selection`, `Visible` through `entity_ref`; a private window reports the app alone; password and PIN fields are never reported |
| voiced `serve` | 1 | fill wave 2, after the PipeWire line (spike V-A): `Begin` from the shell role only, capture through `choose_capture`, unicast signals, an inferd session through porter-client |
| companiond, readerd and voiced binaries | | skeletons that exit 2. companiond's library serves `Companion1` (`serve_on`); its `main` waits for the daemon wiring (a planner model over the bus, the intents transport, the config) intentd serves (F3): see "The bus path" below |

Total: 1 + 4 + 2 + 4 + 1 + 1 = 13 after w4-companion (actions-mcp and companiond are filled).

## Ignored tests

None: every test that waited for a fill runs.

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
3. **`tool_schema` renders directly** (ask 43). stoker's `Shape::to_json_schema` was a stub when
   this was filled, and `Shape` has no object-with-handle form for entities; the schema is built
   from `serde_json` and pinned by `tool_schema_snapshot`. When stoker fills it, the leaf types
   (text, integer, choice) may route through `Shape`, and `Shape` needs an entity-or-handle form.
4. **A target's Space is not known** (ask 42). An entity id carries no Space, so every target
   counts as `Same` (and `Unbound` for an action on nothing): `cross_space_target_asks` waits for
   an entity that names its Space (a `Space` on `EntityId`, or a resolver seam). No corpus case
   for it until then.
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
  name with `DoNotQueue` (a second intentd stops); `serve(router)` is the same on the session bus with the
  shipped configuration. `tests/bus_members.rs` sends every member over the bus and in process and requires the
  same reply.
- **Identity** (`intentd/src/peer.rs`): from the bus's own credentials for the connection: the same user, the
  well-known names it owns, and for a process that owns none, the executable behind its pid (`quire-do` is
  `org.quire.Do`). Roles are `intentd.toml`'s (`dist/intentd.toml` is the shipped default; a file in
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

Not built yet, and what each blocks:

1. **The audit queue is drained and dropped** (`record_of` and the memoryd link are `todo!()`): nothing a call
   does is kept past the process. A person's log of what the terminal did does not exist yet.
2. **intentd does not host `org.quire.Memory` and `org.quire.Companion`** (their providers' `perform` is
   `todo!()`), so their manifests are not installed and `quire-do apps` does not list them.
3. **The reviewer, the policy writer and the reader are built and not working** (`InferdModel`, `InferdWriter`,
   `ReaderClient` are `todo!()`): a terminal never reaches them (it never goes to review and has no task
   policy), but the companion's calls and `Session.Turn` will panic in the request's task until they are filled.
4. **`AppFault` has no "unavailable"** (ask): `AppLink::perform` cannot say the app is not there, so
   `DbusLink::perform` answers `AppRefusal::Failed("the app is not available")` (exit 5), where cli.md section 4
   says 6. Ask: `AppFault::Unavailable`, which `call_step` turns into `CallRefusal::AppUnavailable`.
5. **`serve`'s signature has no roles** (ask): `serve(router)` uses the shipped configuration; the daemon calls
   `serve_on(connection, router, config)`. Ask: drop `serve` or give it the configuration.
6. **sill** must serve `Confirm1` and draw `OnceOrFromTerminal` (ask 3), and `quire-do` over a login needs the
   `Control.Resume` button (item 6 above).
7. The signals `ManifestChanged`, `Hits`, `JournalChanged`, `BreakerTripped` and `Arrived` are declared and not
   emitted; `Halted` and `Resumed` are. `Request.Proceed` answers `Malformed` (no computer-use lease yet).

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

Open, and the asks they make (nothing was edited in the other crates beyond the additions above):

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

## Upstream asks

Nothing below was edited in the other repos. Asks 1 to 4 of the freeze are cleared: almanac's
`RecentEntry` carries `body` under `BodyMode::Json` (companiond reads it through `RecentSource`),
almanac-client's `almanac-service` is behind `in_process`, quire's `ds-intents` has the voice
marks and re-exports `Tally` (docket-ds no longer reaches `ds-core`), and quire's
`docs/workspace-deps.toml` has `pipewire = "0.10"`.

1. **porter `prov` fills** are done (`Label::{trusted_user, untrusted, join}`); docket's tests and
   fakes use them. Labels with an app source and no private scope (`Label` with
   `Source::App`, a model's text) still have no constructor and stay literals.
2. **stoker `model-provider::Shape`** `to_json_schema`, `to_gbnf`, `to_regex` and `check` are
   `todo!()` (see the router items): rendering it for the reader, the reviewer and the policy
   writer waits.
3. **cua `cua-bus`** (stage 4) does not exist: the restart rebuild's computer-use input is
   `ReplayWhat::Run`, which the caller maps from `CuaRecord`; it names no cua type, and
   `companiond::recover` does not read it from `Recent` yet.
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
called until the first session, so an absent inferd is `Unreachable` at the first `open`. The
companiond, readerd and voiced `main`s are still skeletons (exit 2) and every other `serve` body (and the models', planner's and
reader's bodies) is `todo!()`: only the constructors are wired, and each is tested on a private bus
(`tests/inferd_link.rs`). Whoever fills `serve` builds the connection first and passes it here.
