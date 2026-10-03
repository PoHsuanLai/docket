# Findings

Open items and standing facts. An entry names the condition that closes it. After fill wave 1
and the docket amendment there are **36 `todo!()` bodies** in library and daemon code (35 lines:
`MemoryProvider` and `CompanionProvider` share one macro line), listed below, one row per crate
or file; the tests contain none, and no test is `#[ignore]`d. (The freeze had 37: `agent_step`
was filled in fill wave 1.)

## Stubs behind frozen interfaces

| Where | Count | Closes when |
| --- | --- | --- |
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

Total: 2 + 1 + 1 + 7 + 4 + 2 + 2 + 2 + 1 + 2 + 5 + 1 + 4 + 1 + 1 = 36.

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
5. **Reviewer requests use `DataClass::Notes`** (ask 16) as the stand-in for "the person's own
   words", because under the proposed AI policy it has the on-device floor where `AppOwn` may go
   anywhere. A dedicated class (a `Prompt` class in porter's `DataClass`) would say it better.
6. **The reviewer's schema is hand-written** (ask 14) until stoker's `Shape::to_json_schema` is
   filled; `InferReviewer` then switches to `verdict_shape(stage)`.
7. **Stage deadlines come from the clock.** `Clock::after(Millis)` is the router's timer: it races
   every `Reviewer::review`. `SystemClock` sleeps on tokio; the fake `FixedClock` completes only
   for `Millis(0)`, so a test that wants a timeout gives the stage no time and a reviewer that
   hangs (`ScriptedReviewer::hanging`).

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
