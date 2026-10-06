# Architecture

docket is the desktop's action layer and the companion that uses it: **typed actions** every app
declares in a manifest, the **router** that is the one decision point for what an agent may do
(Cedar rules, a per-task policy derived from the person's own words, a reviewer that can only
tighten, a breaker, a confirmation only the person can answer), the **MCP edge**, and **the
companion agent**: one identity over many tasks, each its own session, with a working-set
assembler, a roster, side-conversation capture, an idle pass, restart recovery, and **voice**
(hold-to-talk and dictation). The design is the program spec (`SPEC.md` in the agent-spec
folder, which wins where it differs) with the gating research, the persistent-agent research and
the voice area spec; this file is the map of the code that freezes those interfaces.
`CONVENTIONS.md` holds the rules; `FINDINGS.md` the open items and every `todo!()`.

Reading order: section 1 (find the crate), section 3 (find the home), section 4 (find the
trait), section 5 (the gate in one picture), section 6 (copy the recipe).

## 1. Crates and allowed edges

| Crate | Purpose | I/O |
| --- | --- | --- |
| `docket-core` | the vocabulary: ids, the manifest and `validate` (an indexed kind must declare `<kind>.open`, see FINDINGS "f4-docket-3"), values and `conforms`, the JSON-to-`Args` reader both the companion and the MCP edge use (`args_from_json`), context and its handle-substituted view, previews, calls and every way they end, the undo journal, confirmation, budgets (`charge`, `halted`), the task policy, the planner and reader contract, the roster and episode lines, tasks and the episode skeleton, messages (the draft, the delivery), audit records, `AgentConfig` and the settings rows, the `Intents1` wire, caller roles, summon and voice intents | none |
| `docket-skills` | skills: the `skill.toml` and `SKILL.md` format and its validation, discovery from directories the daemon hands in (`Roots`), reach against the registered manifests, preselection by `when`, the load cap, `uses_first` | directory reads only |
| `policy-point` | the Cedar schema and default policies (`policy/`), `PolicyRequest`, `Pdp::load` (strict validation) and `Pdp::decide` (stubbed) | cedar-policy only |
| `action-review` | the stripped `ReviewRequest`, `ReviewVerdict`, the cascade (`plan`, `escalate`, `tighten`), `ReviewerSet`, the `Reviewer` trait, `InferReviewer` over porter-infer's `Model`, `verdict_shape`, the denial `Breaker` | none |
| `docket-router` | the pure router: `permits` and `acting_role`, `gate`, `Watch` (what a requester may see and say while its request is in flight), the companion's notes, recall and policy narrowing, `session_step`, the `HandleTable` and `planner_view`, `index_step`, the `UndoJournal`, the `Registry`, `assemble` and `inbound_line` for messages, tasks, `call_step`, `Router<S: Seams>` and its seams | none (seams are passed in) |
| `companion-wire` | the bodies of `org.quire.Companion1`: `AskWire`, `AnswerWire` and its cards, plans, forms and refusals, `FrontTask`, and the `SessionRecord`s companiond stores | none |
| `agent-loop` | the companion's pure machines: `assemble`, `agent_step` (inputs include `Messaged`, a request landing in an idle task), `choose_tier`, `side_step`, `idle_step`, `completion_line`, `rebuild`, `front_step` | none |
| `docket-dbus` | `org.quire.Intents1` (ten interfaces), `IntentProvider1`, `Confirm1`, `Companion1` (+ `.Answer`) and `Reader1` as zbus proxies and skeletons, `introspection`, `IntentsError`, bus names and paths, `session_connection` (the session bus of a daemon, from its environment); with feature `inferd`, `inferd_transport`, the one constructor of every daemon's inferd link | zbus |
| `docket-client` | the app side (`IntentProvider`, `ContextSource`, `SummonTarget`, `serve`, `serve_on`: `IntentProvider1` on the app's own name, answering intentd alone) and the caller side (`Intents` over a `Transport`: `InProcess`, `DbusTransport` behind feature `dbus`, whose `connect` finds or activates intentd and whose `call` carries all 34 members, and `Transport::watch` / `Intents::gate_check_watched` (a gate check whose `Progress(Confirming)` is heard and whose `Proceed` and `Close` are said) and `Intents::perform_watched` (a `Run.Perform` whose `Progress` is heard: `Reviewing`, `Previewing`, `Confirming(id)`, `Dispatched`; nothing waits for the caller); `requested` waits for a Request object's `Response`) | per transport |
| `docket-fake` | test only: fixture manifests, `FakeMail`, `FakeFiles`, `ScriptedConfirmer`, `ScriptedReviewer`, `ScriptedWriter`, `ScriptedReader`, `FakeMemory`, `FixedClock` (a virtual clock: `advance`, `asked`, `next_ask`), `RecordingSink`, `MemoryGrants`, `FakeSeams`, `fake_router` | none |
| `docket-testbus` | test only: `PrivateBus` (a private session bus from a scratch config; the daemon is killed by PID, and waited for, when it drops, and by a watchdog if the test process is killed) and `Reaped`, the guard under it; every daemon's bus tests use it | zbus |
| `docket-accept` | test only: the agent tier's end-to-end acceptance. Thin mains run intentd, companiond and readerd as processes (`accept-*`); `build.rs` builds porter's `inferd` and almanac's `memoryd` (feature `test-keys`) from their own workspaces into `<target>/accept-siblings`, and inferd plays the cassettes in `dev/accept/cassettes`; the library holds the `Confirm1` server, the mail provider, the world (private bus, scratch dirs, event-driven waits) and the launcher's calls. `dev/accept/` holds the fixtures, README and `run.sh` | intentd, companiond, readerd, docket-testbus (inferd and memoryd by build.rs) |
| `docket-eval` | the red-team suite: the corpus format and loader (`eval/`), `Case` (`Driver`: the companion or a terminal), `Expect`, `RunReport`, `Metrics`, `wilson`, the runner skeleton and `Harness`; and the conformance check `docket-eval --check-app <dir>` (`check`, `ui`) | none |
| `docket-cli` | `quire-do`: every app drivable from a command line generated from its manifest. A thin client over `docket-client` with caller role `cli`: the command grammar, the parameter mapping, `describe` through stoker's `Shape::to_json_schema`, the exit codes, text and JSON output, shell completion. It reaches the bus only through `docket-client`, and never the router, Cedar or the policy point | per transport (the binary: the session bus) |
| `actions-mcp` | the MCP edge: `tools`, `tool_name`, `hints_of`, `mcp_label`, `McpExpose` (the setting `agent.mcp.expose`, off by default; `settings.rs` reads `docket/settings.toml`), `read_call` (docket-core's `args_from_json`), `McpFault`, `McpEdge` over `rmcp`, which serves `list_tools` and `call_tool`; the binary (`McpConfig`, `claim`, `start`, `run`): stdio or `--socket`, off unless the settings file says `agent.mcp.expose = "on"`, one bus name (`org.quire.ActionsMcp`, the `mcp` role) | rmcp |
| `intentd` | the daemon and its library: `IntentdConfig` (and the shipped `dist/intentd.toml`), the built-in `org.quire.Memory` and `org.quire.Companion` providers and `HostedLink` (which answers them in process), `AlmanacMemory`, `QueuedSink` (bounded), `record_of` and `AuditLog` (the audit trail into memoryd), `DbusLink`, `SheetConfirmer`, `FileGrants`, `InferdModel`, `InferdWriter`, `ReaderClient`, `SystemSeams`, `Peers` (who is on a connection: names, else the cgroup read through porter's `ProcCallers`; `ProcRoot`, the test-only `INTENTD_PROC_ROOT`), `serve` / `serve_on` (one handler per `Intents1` interface), `signals` (`Marks`, `changes`, `pump`: the signals that say the router's state changed), `watch_logind` (the end of the person's session), `start` / `run` (the daemon) | everything |
| `companiond` | the companion daemon: `Companiond` (one identity over many tasks: `TaskRuntime`, the working set `sources`, the loop's effects `drive`, a pressed card `act`, messages `inbox`, episodes `finish`, the idle pass `idle`, restart `resume`), `PlannerModel` over a `Catalogue`, `Shared` (what the bus reads), `recover` (and `RouterRecent`, its `RecentSource` over the router), `completion_effects`, `CompaniondConfig`, `start` / `run` (the daemon), `serve` and `serve_on` | everything |
| `readerd` | the quarantined reader, a separate process: `ReaderHost` (the one place the reader key is made), `reader_request`, `ReaderService`, `serve`, `start` / `run` (the daemon) | everything |
| `docket-ds` | the adapter quire apps use: chips and keep, things and labels, summon answers, `DsContextSource`, `DsSummonTarget`, the voice bridge | none |
| `voice-wire` | the bodies of `org.quire.Voice1`: `VoiceBegin`, `VoiceEvent`, `UtteranceEnd`, `VoiceStatus`, `VoiceRefusal` and its 1:1 error names, `SpeakWire` | none |
| `voice-loop` | the voice machines: `utterance_step` (the microphone is open exactly in Opening, Listening and Tail), `speech_step` (barge-in), `sentences`, `PcmBuffer` | none |
| `voiced` | the microphone's owner: `VoicedConfig`, the `AudioDevice` seam and `choose_capture` (never a monitor), the `Voice1` skeleton and its introspection, `serve` | everything |

Allowed direct edges (checked by `scripts/check-boundary.sh`; dev-dependencies are outside it):

| Crate | May depend on |
| --- | --- |
| `docket-core` | `prov`, `porter-core`, `almanac-core`, `cua-action`, `model-provider` |
| `docket-skills` | `docket-core`, `prov`, `porter-core`, `toml` |
| `policy-point` | `docket-core`, `prov`, `porter-core` |
| `action-review` | `docket-core`, `prov`, `porter-core`, `porter-infer`, `model-provider` |
| `docket-router` | `docket-core`, `docket-skills`, `policy-point`, `action-review`, `prov`, `porter-core`, `almanac-core` |
| `companion-wire` | `docket-core`, `prov`, `porter-core`, `porter-infer`, `almanac-core` |
| `agent-loop` | `docket-core`, `companion-wire`, `almanac-core`, `porter-core`, `prov` |
| `docket-dbus` | `docket-core`, `prov`, `porter-dbus`; `porter-client` with feature `inferd` |
| `docket-client` | `docket-core`, `docket-router`, `prov`; `docket-dbus` with feature `dbus` |
| `docket-fake` | `docket-core`, `docket-router`, `docket-client`, `policy-point`, `action-review`, `prov`, `porter-core`, `almanac-core` |
| `docket-testbus` | `docket-dbus` |
| `docket-accept` | `almanac-client`, `almanac-core`, `companion-wire`, `companiond`, `docket-client`, `docket-core`, `docket-dbus`, `docket-router`, `docket-testbus`, `intentd`, `porter-core`, `porter-infer`, `prov`, `readerd` |
| `docket-eval` | `docket-core`, `docket-skills`, `docket-fake`, `docket-router`, `prov`, `porter-core` |
| `actions-mcp` | `docket-core`, `docket-client`, `docket-dbus`, `prov`, `porter-core` (+ `rmcp`) |
| `intentd` | `docket-core`, `docket-skills`, `docket-router`, `docket-client`, `docket-dbus`, `policy-point`, `action-review`, `prov`, `porter-core`, `porter-infer`, `porter-client`, `almanac-core`, `almanac-client` |
| `companiond` | `agent-loop`, `almanac-core`, `companion-wire`, `docket-core`, `docket-skills`, `docket-client`, `docket-dbus`, `prov`, `porter-client`, `porter-core`, `porter-infer` |
| `readerd` | `docket-core`, `docket-client`, `docket-dbus`, `prov`, `porter-client`, `porter-core`, `porter-infer` |
| `docket-cli` | `docket-core`, `docket-skills`, `docket-client` (feature `dbus`), `model-provider`, `prov`, `porter-core` |
| `docket-ds` | `docket-core`, `docket-client`, `companion-wire`, `voice-wire`, `prov`, `porter-core`, `ds-intents` |
| `voice-wire` | `docket-core`, `porter-core`, `porter-infer` |
| `voice-loop` | `voice-wire`, `docket-core`, `porter-core`, `porter-infer` |
| `voiced` | `voice-loop`, `voice-wire`, `docket-core`, `porter-core`, `porter-infer`, `porter-client`, `speech-vad` |

The repo order is stoker, porter, almanac, docket, cua, sill. docket reaches stoker's pure
crates (`cua-action`, `model-provider`, `speech-vad`), porter (`prov`, `porter-core`,
`porter-infer`, `porter-client`, `porter-dbus`), almanac (`almanac-core`, `almanac-client`) and
quire's view-only `ds-intents` (through `docket-ds` alone), all by sibling path until pinned git
revs replace them. Nothing of `cua` or `sill` may enter any tree here.

`docket-cli` (`quire-do`) reaches the bus only through `docket-client`: no direct `docket-dbus` or `zbus`, and none of `docket-router`, `policy-point`, `action-review` or `cedar-policy` anywhere in its tree. A terminal has no way around intentd's gate.

External boundaries: see `scripts/check-boundary.sh` (the rules and their reasons are written
there). `zbus` is behind `docket-dbus` and `docket-client`'s `dbus` feature only; the pure set
never reaches an effect crate; `cedar-policy` only through `policy-point`; `rmcp` only in
`actions-mcp`.

## 2. Modules

| Crate | Modules |
| --- | --- |
| `docket-core` | `units`, `ids` < `value`, `args`, `manifest` < `validate`, `schema` < `context`, `preview` < `call`, `undo`, `grant`, `confirm`, `review`, `budget` < `task_policy`, `reader`, `planner`, `roster`, `skill` < `message`, `task`, `audit`, `gate`, `index`, `summon` < `config`, `caller`, `wire`, `when` |
| `docket-skills` | `fault`, `skill` < `discover`, `library` |
| `policy-point` | `request` < `pdp` |
| `action-review` | `verdict` < `request`, `breaker` < `cascade`, `infer` |
| `docket-router` | `auth`, `registry`, `session`, `index`, `journal`, `handles`, `messages`, `tasks` < `gate`, `call` < `seams` < `state`, `labels`, `argcheck`, `consent`, `coverage`, `who`, `companion`, `skills` < `prepared`, `prepare`, `driven`, `confirm`, `perform`, `finish` < `policy`, `terminal`, `dryrun`, `opening`, `reading`, `messaging`, `search`, `control`, `gatecheck`, `watch`, `notes`, `recall` < `router` |
| `companion-wire` | `ask`, `answer` < `record` |
| `agent-loop` | `tier`, `front`, `completion`, `side`, `idle`, `rebuild`, `assemble` < `step` |
| `docket-dbus` | `names`, `error`, one file per interface, `introspect` |
| `docket-client` | `provider`, `transport` < `watch` < `awaiting`, `watch_bus`, `watch_in_process` < `bus` < `intents`, `session_calls`, `provider_bus` < `serve` |
| `docket-fake` | `labels`, `simple`, `mail`, `files`, `scripted`, `seams`, `router` |
| `docket-testbus` | `guard` < `lib` (`PrivateBus`) |
| `docket-accept` | `provider`, `confirm` < `world` < `drive` |
| `docket-eval` | `case`, `report`, `corpus`, `block`, `world`, `steps`, `runner`, `metrics`, `check` < `ui` (the binary `docket-eval` runs `check`) |
| `docket-cli` | `exit`, `args` < `resolve`, `when` < `params`, `schema`, `outcome` < `render`, `complete`, `help` < `exec` < `lib` (`run`), `main` |
| `intentd` | `config`, `builtin_memory`, `builtin_companion` < `builtin`, `record` < `sink` < `audit`, `memory`, `grants`, `procroot` < `peer` < `sheet`, `link`, `infer`, `writer`, `reader_client`, `reviewers`, `system`, `bus` (`request`, `query`, `run`, `session`, `control`) < `serve`, `logout`, `manifests` < `signals` < `daemon` (`main`) |
| `companiond` | `clock`, `fault`, `shared`, `catalogue` < `args`, `render` < `planner`, `task` < `runtime`, `completion`, `recover`, `sources`, `drive`, `act`, `inbox`, `finish`, `idle`, `records`, `resume`, `serve`, `config` < `daemon` |
| `readerd` | `host`, `request`, `answer` < `service` < `serve` < `daemon` (`main`) |
| `docket-ds` | `chips`, `things`, `summon`, `context`, `voice` |
| `voice-wire` | `text`, `begin`, `event`, `status`, `refusal` |
| `voice-loop` | `buffer`, `sentencer`, `utterance`, `speech`, `coordinate` |
| `voiced` | `names`, `config`, `device`, `bus`, `introspect`, `serve` |

## 3. One home per concept

| Concept | Home |
| --- | --- |
| who acted, effects, labels, ids of sessions, runs, tasks, messages and entities, the confirmation receipt, `Message`, `AgentRef`, `Address` | porter `prov` (never redefined here) |
| the manifest and `validate`, `ActionDecl`, `ParamDecl`, `ArgSink`, `Value`, `Args` | `docket-core::{manifest, validate, value}` |
| the four rulings of policy (`Ruling`), the reasons to ask (`AskReason`), the coarse `DenyCode`, `Stage`, `Impact`, `ReasonCode`, `BreakerTrip`, `Strictness` | `docket-core::review`: the refusals and the audit name them, so they sit below the crates that decide |
| the reviewer's `ReviewVerdict`, `tighten`, `plan`, the denial `Breaker` | `action-review` |
| the Cedar schema, default policies, `Pdp` | `policy-point`, files in `policy/` |
| the terminal's rows of the grid (`principal.kind == "cli"`) and its standing grant | `policy/default.cedar` (`cli-asks`, `cli-destructive-asks`, `cli-granted-final`); the grant itself is `docket-router::terminal`: a task policy of the terminal's session naming the action, given only by `ConfirmAnswer::AllowedFromTerminal` |
| `quire-do`: the command grammar, the parameter mapping, the exit codes | `docket-cli` (`args`, `params`, `exit`); a terminal's arguments are labelled `Untrusted, Source::Cli` by the router, never by the client |
| the conformance check of every app repo | `docket-eval --check-app` and `scripts/check-intents.sh` (cli.md section 6) |
| the planner's view and its builders: the view type | `docket-core::planner`; the one builder, `planner_view`, and the `HandleTable`, `docket-router::handles` |
| the roster and episode lines the planner reads | `docket-core::roster` (the view holds them) |
| episodes and their skeleton | almanac `Episode`; docket builds the skeleton (`docket-core::task::skeleton_of`) and the router records it |
| the one message model | porter `prov::Message`; docket's `MessageDraft`, `Delivery`, `InboundLine` are input and views |
| the task policy, `compare`, `covers`, `PolicyWriter` | `docket-core::task_policy` |
| the session budget, ledger, kill switch | `docket-core::budget` (`cua-run`'s per-run budget is cua's) |
| the `Intents1` wire and who may call which member | `docket-core::{wire, caller}`; the table `docket-router::auth` |
| the interfaces as D-Bus | `docket-dbus` and `dbus/*.xml`; `Voice1` is `voiced`'s |
| the planner loop, the assembler, restart | `agent-loop` |
| the companion's wire (answers, cards, session records) | `companion-wire` |
| settings values | `docket-core::config` (`AgentConfig`, `SETTING_ROWS`); the keys are sill's |
| turns and who may record one | `docket-core::planner` (`TurnIn`, `UserTurn`, `TurnSource`, `TurnVia`); the role table in `auth` |
| structured-output shapes | stoker `model-provider::Shape`; `ValueSchema::shape` maps into it, inferd owns repair and retry |
| computer-use actions | stoker `cua-action`; `CuaAsk` is the gate's input |
| views of the above for quire apps | quire `ds-intents`; `docket-ds` converts |

## 4. Traits

| Trait | Home | Implemented by |
| --- | --- | --- |
| `Seams` | `docket-router` | `SystemSeams` (intentd), `FakeSeams` (docket-fake) |
| `AppLink`, `GrantStore`, `EventSink`, `Clock`, `MemoryLink` | `docket-router` | `HostedLink` (over `DbusLink`), `FileGrants`, `QueuedSink`, `SystemClock`, `AlmanacMemory`; the fakes |
| `Confirmer` | `docket-core` | `SheetConfirmer`, `ScriptedConfirmer` |
| `Reviewer` | `action-review` | `InferReviewer`, `ScriptedReviewer` |
| `PolicyWriter` | `docket-core` | `InferdWriter`, `ScriptedWriter` |
| `Reader` | `docket-core` | `ReaderService` (readerd), `ReaderClient` (intentd's end of `Reader1`: it names the session, readerd resolves the handles through it), `ScriptedReader` |
| `IntentProvider`, `ContextSource`, `SummonTarget` | `docket-client` | apps; `DsContextSource`, `DsSummonTarget`; the built-in providers; `FakeMail`, `FakeFiles` |
| `Transport` | `docket-client` | `InProcess`, `DbusTransport` |
| `RecentSource` | `companiond` | `RouterRecent` (`Session.Recall` through intentd's router: a body only for a trusted entry), a fake |
| `PromptHost`, `DictationBridge`, `WindowFacts` | `docket-ds` | quire apps |
| `AudioDevice` | `voiced` | the PipeWire device (stubbed), `FakeAudioDevice` |

Closed sets stay enums: requests, replies, rulings, refusals, records, every machine's states,
inputs and effects.

## 5. The gate for one call, in one picture

```
caller (role from the connection) -- permits(role, member) ----------------------- refuse the request
  -> Run.Perform(call)
       arguments checked against the manifest ---------------------------------- BadArgs
       handles resolved, labels kept; SessionSaw and per-sink integrity computed by the router
       gate(): halt -> budget -> exact repeat -> consent denied -> Ruling
         Ruling from Cedar (rule-of-two and untrusted-sink are forbids no layer can lift):
           Deny ................. Refuse(Denied(code))      (the planner sees only the code)
           Ask .................. Previewing -> Confirming  (the app's dry run; the person answers)
           AllowJudged .......... reviewer cascade (Quick, Deliberate, SecondOpinion), tighten():
                                    every planned stage allows -> Run; any Ask or failure -> Confirm;
                                    any Deny -> Refuse
           AllowFinal ........... Run
       Confirming: a receipt only a hardware or shell input can make; voice never answers
       Dispatched -> app Outcome -> journal the undo, audit one record, breaker notes the decision
  breaker: 3 in a row, 10 of the last 50, or one goal with different arguments -> session Paused
           until the person says something
```

A message never skips this: a `Request` in a `prov::Message` is evaluated under the receiver's
own task policy and the same pipeline. The label travels with the message and joins into the
receiver's taint; a message across Spaces is delivered and never grants a memory read.

## 6. Recipes

**Add an action to a built-in provider.** Edit `manifests/org.quire.<App>.toml` (every field
written; names carry the app's prefix; an outbound action declares a recipient or destination
sink; a write without undo is destructive). `docket_core::validate` and the router's
`Registry::unresolved` check it in `tests/manifests.rs`; add its behaviour to the provider:
`intentd::builtin_memory` for `org.quire.Memory`, `Router::companion_perform` (docket-router,
`companion.rs`) for `org.quire.Companion`. `HostedLink` routes the two names to them.

**Add a member to `Intents1`.** (1) A variant of `IntentsRequest` and its reply in
`docket-core::wire`; (2) `Member` and `IntentsRequest::member`; (3) its row in `docket-router::auth`
(who may call it, and a test that names the guarantee); (4) the method in `docket-dbus`'s table
for its interface; run `cargo test -p docket-dbus`, which prints the new XML; replace
`dbus/org.quire.Intents1.xml` with it in the same commit; (5) a method on `docket_client::Intents`.

**Add a Cedar rule.** Write it in `policy/default.cedar` with an `@id`; `Pdp::load` validates it
strictly against `policy/quire.cedarschema`; add the row to `policy-point/tests/grid.rs`. A rule
that tightens only is a forbid; a user override directory may only forbid.

**Add an action to every face.** Declare it in the app's manifest (recipe above) and implement it in the app's `IntentProvider`. That is all: `quire-do <app> <action>` (`docket-cli`), the MCP tool (`actions-mcp`) and the D-Bus `Run.Perform` are generated from the declaration. An app's menu command or shortcut that is the face of the action names it in `<AppName>.ui.toml`; `scripts/check-intents.sh` fails the app's gate for a menu command with no action that is not listed UI-only.

**Add a red-team case.** A TOML file under `eval/<corpus>/`, in the format documented in
`docket-eval/src/case.rs`: the person's turns, the world, the scripted calls and messages of a
hijacked or naive planner (each argument says where it came from), and one `Expect`. Every case
carries a `why`. `corpora_parse` loads them all.

**Add a setting.** A field of `AgentConfig` with its default, a row in `SETTING_ROWS` and an arm
of `AgentConfig::value`; the test that pins the rows to the defaults fails until all three agree.
The design/22 row and the sill key are the other repos' agents'.

**Add a machine.** A pure `(state, input) -> (state, effects)` in `agent-loop` or `voice-loop`,
every type `Serialize + Eq`, a table test, and the effects carried out by a daemon.

## 7. Repo rules

- Licence `MIT OR Apache-2.0`; edition 2024; `unsafe_code = "deny"`; `rust-toolchain.toml` pins
  1.98.1 and `deny.toml` is the licence gate.
- Cross-repo dependencies are `path` entries to sibling checkouts (`../porter`, `../almanac`,
  `../stoker`, `../quire`) until each upstream freeze is merged and a pinned git rev replaces them.
  External versions are copied from quire's `docs/workspace-deps.toml`, never chosen here.
- Behaviour is `todo!()` behind frozen signatures, each listed in `FINDINGS.md`; shape tests
  (round trips, pinned JSON, introspection, tables) pass.
- Tests never touch the real system: no real bus, no real apps, no GPU, no real engine, no network.
  `voiced`'s binary is a skeleton that exits with code 2 (its `serve` is a stub); intentd, companiond and readerd serve, and each has a binary test on a private bus.
- The gate, with every exit code checked (`scripts/gate.sh` runs exactly this):

  ```bash
  set -euo pipefail
  cargo fmt --all --check
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo test --workspace --all-features
  cargo test -p policy-point -p docket-eval
  ./scripts/check-boundary.sh
  cargo deny check licenses
  echo "GATE GREEN"
  ```

  No crate is excluded: every crate builds and tests without hardware or network. (`voiced`
  holds the PipeWire device behind a seam and `speech-vad-silero` is not a dependency, so
  neither needs the exclusion `recall-fastembed` needs in almanac.)
