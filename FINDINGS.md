# Findings

Open items and standing facts. An entry names the condition that closes it. At the freeze there
are **37 `todo!()` bodies** in library and daemon code (49 lines: `MemoryProvider` and
`CompanionProvider` share one macro line), listed below, one row per crate or file; the tests
contain none, and no test is `#[ignore]`d.

## Stubs behind frozen interfaces

| Where | Count | Closes when |
| --- | --- | --- |
| agent-loop `agent_step` | 1 | fill wave 1 (F1d): the planner loop table (Idle, Planning, AwaitingCalls, AwaitingReader, Paused, Finished); un-ignore `a_refusal_tells_the_planner_only_the_coarse_code_and_a_trip_pauses_the_loop` |
| docket-client `DbusTransport::call`, `serve` | 2 | fill wave 1 (F1d) with docket-dbus's codec: one match from `IntentsRequest` to the member and its JSON arguments; `IntentsError` maps back; the Request object's `Response` carries the reply of Perform, Undo, Widen and Check; `serve` exports `IntentProviderSkeleton` over the app's seams |
| actions-mcp `McpEdge::call` | 1 | fill wave 2: tool name to action, JSON arguments to `Args` by `ParamType` each labelled with `mcp_label`, `Intents::perform`, outcome or coarse refusal to a tool result |
| intentd `record_of` | 1 | fill wave 2: `AuditRecord` to almanac `Record` (typed `Message` and `Episode` bodies, the rest `Area { Docket }` with the things each names for cascade-forget) |
| intentd `DbusLink::{perform, dry_run, undo, context, search, preview, suggest}` | 7 | fill wave 2: `IntentProviderProxy` on the app's own name after checking its owner derives to the same `AppId`; the 250 ms, 5 s and progress-request latencies |
| intentd `InferdModel::{chat, embed}`, `InferdWriter::derive`, `ReaderClient::extract` | 4 | fill wave 2, blocked on porter's `DbusTransport::open` and session fills: sessions through porter-client; `ReplyShape::Json` of the policy record from the person's turns and the catalogue alone; `Reader1.Extract` |
| intentd `FileGrants::{grants, record}` | 2 | fill wave 2: a missing or malformed file is no grants and never a panic; atomic write |
| intentd `SheetConfirmer::{confirm, cancel}` | 2 | fill wave 2: `Confirm1`; a vanished sill ends the confirmation as dismissed, never an allow |
| intentd `MemoryProvider::perform`, `CompanionProvider::perform` | 2 | fill wave 2: `memory.recall/facts/propose/forget` through memoryd as `Caller::Router` (untrusted proposals land pending); `companion.task.start` opens a child session and records `task.started`, `companion.task.message` goes through message delivery |
| intentd `serve` | 1 | fill wave 2: one handler per `Intents1` interface, identity and role derived from the connection, Request objects for Perform, Undo, Widen and Check |
| companiond `PlannerModel::{request, plan}` | 2 | fill wave 2: the sections in the assembler's order into messages, `ToolDecl` per action card, a pinned session so the cached prefix is reused |
| companiond `Companiond::{open, ask, arrived, roster, tick}` | 5 | fill wave 2: the entry points of the bus over `agent_step`, `side_step`, `idle_step`, `completion_effects` and `recover` |
| companiond `serve` | 1 | fill wave 2: `Companion1` and `Companion1.Answer`; recover and open a fresh session for the front task on start |
| readerd `reader_request`, `ReaderService::{extract, extract_in}`, `serve` | 4 | fill wave 2, with stoker's `Shape::to_json_schema`: the fenced data, no tools, the schema as the reply shape, `conforms` on the answer |
| docket-ds `DsContextSource::snapshot` | 1 | fill wave 2 (quire apps): `ContextModel` to `Here`, `Selection`, `Visible` through `entity_ref`; a private window reports the app alone; password and PIN fields are never reported |
| voiced `serve` | 1 | fill wave 2, after the PipeWire line (spike V-A): `Begin` from the shell role only, capture through `choose_capture`, unicast signals, an inferd session through porter-client |
| daemons serve their bus | | the items above; every daemon binary is a skeleton that exits 2 |

Total: 1 + 2 + 1 + 1 + 7 + 4 + 2 + 2 + 2 + 1 + 2 + 5 + 1 + 4 + 1 + 1 = 37.

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
the corpus (8 cases) loads, `wilson` and `judge` tables. actions-mcp: tool names, hints, offered
actions, `mcp_label`. intentd: the configuration, the built-in manifests, record kinds, the queue,
the memory seam, the clock, every unit file's sandbox lines. readerd: the fixed instructions, the
reader host. docket-ds: every mark conversion. voice-wire, voice-loop, voiced: see their rows in
`ARCHITECTURE.md` (the utterance and speech machines, the sentencer, the buffer, the introspection).

Filled in fill wave 1 (router): docket-core `compare`, `covers` and `intersection` (a policy against
another, a call against a policy, a child against its parent) with the whole-label and
whole-segment pattern match, and `tool_schema` (one property per parameter, a handle accepted where
text, an entity or a file goes). docket-router: `call_step` (every row of the lifecycle table),
`Router::handle` (all 29 members), `child_policy`, `roster_of`, the labels the router derives
itself (a model's value is trusted only when it traces to the person's turns, a thing the router
showed the session, or a closed set), consent per data class (a tainted session cannot lean on an
`Always` for a write), coverage, the task policy (derived from the person's turns, capped by a parent
and by a prompt field, widened only on confirmation), review, preview, confirmation, dispatch, the
journal, the breaker, halts that withdraw sheets, messages (stamped, labelled, delivered as input),
the shadow index, and the computer-use gate. docket-fake: `clear` and `add_*` on the apps and the
scripted seams, so one harness serves many cases. docket-eval: `run_case` and `run_corpus` over the
fake router with a judge that always allows and the widest policy.

## Router: open items and interface asks

Each names the change that closes it. Nothing below was edited outside docket.

1. **`IntentsRequest::Perform` carries no session.** A companion's call lands in its newest
   session, which is wrong while two of its tasks work at once. Ask: `session: Option<SessionId>`
   on `Perform` (and on the `Run.Perform` D-Bus method), `None` for the person's own surfaces.
2. **`covers` has no `ActionDecl`.** The effect and each argument's sink are the manifest's, so
   `covers(policy, call, labels)` judges what the call shows (state, action, kinds, count, and that
   every untrusted argument matches a trusted pattern) and docket-router's `coverage` checks the
   ceiling and an app's cap first. Ask: `covers(policy, decl, call, labels)` and `Widening::Pattern`
   then names the declared sink instead of reading it from the value's type.
3. **`call_step` cannot make its richest effects.** It holds no call data, so it never emits
   `Audit` or `Confirm`, enters `Confirming` with an unissued id, and ends bad arguments with the
   parameter `args`; `Router` writes the audit when a call ends and draws the sheet when the state
   becomes `Confirming`. Ask: `CallEvent::Previewed` carries the built `ConfirmRequest`,
   `ArgsChecked` carries the `ParamName`, and `AppTimedOut` needs a seam that can say it:
   `AppLink::perform` has no timeout error, so the router never feeds it.
4. **`roster_of` has no `HandleTable`.** A planner's goal is untrusted text, so `TaskRecord` holds
   the `Reveal<String>` made when the task started (a handle in the spawning session's table).
   Ask: none; a handle minted into the reading session's table needs `roster_of` to take it.
5. **`Context.Current` names no app.** The router looks in the window of the last prompt field that
   recorded a turn; a launcher turn names none, so it refuses. Ask: the turn (or `Context`) carries
   the summoning app.
6. **Stored state grew.** `SessionRecord`, `RouterState` and `TaskRecord` gained fields (the ledger,
   the breaker, what the session saw, its turns and policy, per-Space strictness, the shadow index,
   the goal and last step of a task). They are built by `new` and nothing outside docket-router
   constructs one by literal.
7. **`tool_schema` renders directly.** stoker's `Shape::to_json_schema` is a stub when this was
   filled, and `Shape` has no object-with-handle form for entities; the schema is built from
   `serde_json` and pinned by `tool_schema_snapshot`. When stoker fills it, the leaf types (text,
   integer, choice) may route through `Shape`.
8. **Outbound under `Default`.** The shipped Cedar grid (actions-addendum A2) lets an outbound act
   with every sink trusted, inside the task policy, go to review and then run; QUESTIONS S1 says
   outbound always asks under `Default`. The router follows the grid. policy-point's owner decides.
9. **A target's Space is not known.** An entity id carries no Space, so every target counts as
   `Same` (and `Unbound` for an action on nothing): `cross_space_target_asks` waits for an entity
   that names its Space.

## Upstream asks

Nothing below was edited in the other repos.

1. **almanac-core: `Recent` must return bodies.** The restart rebuild reads companiond's own
   session records, the router's task events, messages and episodes through `Recent`, but
   `RecentEntry` carries only a summary, effect, label and text, and an `Area` payload is opaque
   JSON. Ask: a `body` on `RecentEntry`:
   ```rust
   pub struct RecentEntry { pub summary: EventSummary, pub effect: Effect, pub label: Label,
       pub text: Option<UserText>,
       /// The owner's serde JSON for an `Area` payload, or the serde form of a `Message` or `Episode`.
       pub body: Option<JsonText> }
   ```
   (or `RecentQuery { .., bodies: BodyMode { Without, Json } }`). Until then `ReplaySource` is a
   seam and `recover` is built over `ReplayEvent`s; the adapter from `RecentEntry` is the stub.
2. **almanac-client: make `almanac-service` optional.** `almanac-client` depends on
   `almanac-service` unconditionally (for `InProcess`), which links SQLCipher and a vendored
   OpenSSL into every D-Bus client: intentd and, through it, nothing else needs them in process.
   Ask: `almanac-service` behind a default-off feature `in_process`, with `InProcess` and `Backend`
   under it; `dbus` already exists. `scripts/check-boundary.sh` allows intentd to reach `rusqlite`
   for this reason, and forbids it to companiond and readerd.
3. **quire `ds-intents`: the voice marks of voice.md section 3.6.** Not in the crate yet:
   `SummonOriginMark { Keyboard, Voice, Dictation }`, `HeardMark { Level(InputLevel), Tail(String),
   Committed(String), Ended(HeardEndMark) }`, `HeardEndMark { Send(String), Nothing, Cancelled }`,
   `DictationPort`, `DictateSerial(pub u64)`, and `CompanionPort::on_summon` taking the origin.
   `docket-ds` mirrors `HeardMark` as its own `Heard` and `HeardEnd` and converts from `VoiceEvent`;
   it swaps to the quire types when they land. Also: `ds_core::vocab::Tally` (the count a
   `ContextChip` carries) is not re-exported by `ds-intents`, so `docket-ds` takes `ds-core` as a
   direct path dependency (listed in its edge row); a re-export would remove it.
4. **quire `docs/workspace-deps.toml`: `pipewire = "0.10"`** (voice.md section 2.3, spike V-A) is
   absent. docket added no external crate beyond the pinned block (no line was missing otherwise:
   cedar-policy 4.13 and rmcp 3.5 are there, and `cargo deny check licenses` passes on them).
   `voiced` holds the device behind the `AudioDevice` seam; the PipeWire device and a `silero`
   feature on `speech-vad-silero` (an excluded crate in stoker) wait on the line.
5. **porter `prov` fills** (already in porter's FINDINGS): `Label::{trusted_user, untrusted, join}`,
   `Labelled::zip`, `endorse` and `declassify` are `todo!()`. docket's tests and fakes write label
   literals; `intake_label` and any real join wait.
6. **stoker `model-provider::Shape`** `to_json_schema`, `to_gbnf`, `to_regex` and `check` are
   `todo!()`: `ValueSchema::shape` builds the shape, and rendering it for the reader, the reviewer
   and the policy writer waits.
7. **cua `cua-bus`** (stage 4) does not exist: the restart rebuild's computer-use input is
   `ReplayWhat::Run`, which the caller maps from `CuaRecord`; it names no cua type.
8. voice.md section 3.4 (porter's speech items) is all present in `porter-infer`
   (`TranscribeBegin`, `SpeakRequest`, `AudioFrame`, `HeardDelta`, `ClientFrame::{Audio,
   EndOfAudio}`, `Readiness`): nothing is asked of porter for voice.

9. **action-review: the stage timeout needs a timer.** `InferReviewer` is pure and cannot race
   the model against `ReviewTimeouts` (a `0 ms` stage fails at once; a hung model does not). Ask
   (docket interface, not another repo): either `docket-router` wraps `Reviewer::review` in its
   `Clock`-driven deadline (the router already owns `ReviewError::Timeout` handling), or
   `InferReviewer` gains a `Deadline` seam (`fn within<T>(&self, Millis, impl Future<Output = T>) ->
   impl Future<Output = Result<T, Elapsed>>`) that intentd fills with `tokio::time::timeout`.
10. **stoker `Shape::to_json_schema`** (item 6) also blocks `InferReviewer`: it sends the verdict
    record as a hand-written JSON Schema (`record_schema`, pinned by a test against
    `verdict_shape`'s codes); it switches to `verdict_shape(stage).to_json_schema(..)` when the fill
    lands.

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
