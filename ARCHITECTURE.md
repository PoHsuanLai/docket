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
| `docket-router` | the pure router: `permits` and `acting_role`, `gate`, `Watch` (what a requester may see and say while its request is in flight), the companion's notes, recall and policy narrowing, `session_step`, the `HandleTable` and `planner_view`, `index_step`, the `UndoJournal`, the `Registry`, `assemble` and `inbound_line` for messages, tasks, `call_step`, the session's durable record (`Wal`: entries queued under the lock, `Router::flush`, the write-ahead taint `ahead_of_reveal`, `settle`; `Session.Stored` (`stored`: the log read through the router, only the sessions the caller may bring back), `Router::restore_session`, `adopt_sessions`, the pure `rebuild`), the `acp_agent` role (the host of an external coding agent: `Who::agent`, `Voice::Agent` whose arguments are as trusted as a planner that has seen nothing untrusted, program grants, one-use approvals from a permission request the person allowed, the pseudo-app only for that role), `Router<S: Seams>` and its seams | none (seams are passed in) |
| `companion-wire` | the bodies of `org.quire.Companion1`: `AskWire`, `AnswerWire` and its cards, plans, forms and refusals, `FrontTask`, and the `SessionRecord`s companiond stores | none |
| `companion-client` | the person's side of `org.quire.Companion1`: `CompanionTransport` (open, ask, follow the answer object, close) and `DbusCompanion` over the session bus; what `quire-do ask` is written against. Not in `docket-client`, so the apps and cuad that use that crate do not take on the companion's wire | the session bus (`DbusCompanion`) |
| `agent-loop` | the companion's pure machines: `assemble`, `agent_step` (inputs include `Messaged`, a request landing in an idle task, and `ReadFailed`, a read the planner may write again, counted with unreadable replies), `choose_tier`, `side_step`, `idle_step`, `completion_line`, `leaked_call` (a tool call a model left in its words), `rebuild`, `front_step` | none |
| `docket-planner` | the planner model, portable: `PlannerModel<P: porter_client::Transport>` (`converse`: a view in, one `PlannerReply` out), `Catalogue` (the manifests' actions as tools), the prompt (`RULES`, `messages`), `read_call`, `planner_label`, and `quire_read`'s parts read one by one into a `ReadFault` the planner is told. Moved out of companiond so an app-hosted agent asks its own model the same way; the transport is the caller's choice (inferd over D-Bus on the desktop, the latchkey socket or `InProcess` elsewhere) | per transport |
| `docket-tasks` | the companion's task model, portable (moved out of companiond): `Companion<P, I, K, S>` over a model Transport `P`, a router Transport `I` (the app side of `Intents1`), a clock `K: Now` and a surface `S: Surface` (what is told when state changes, how an interactive request cuts into background work, what an answer is called there; `Quiet` is the one that listens to nothing). It holds the front pointer, the roster, `TaskRuntime` (the person's words, steps, handles, inbox, the answer so far, `Failure`), the drive loop's effects (`drive`: plan, call, read, hold, close), the working set (`sources`), side conversations and the idle pass (`tick`, with a model race that needs no runtime: `futures-util`'s `select` over the surface's `interrupted`), the inbox and workers, the episodes a task leaves (`finish`), the native session backend (`NativeBackend`, `NativeHost`, `RouterLog`: the planner loop behind `docket-session`'s `SessionBackend` and `SessionHost`, one `Flight` per turn so a pull is cancel-safe and a call waits at the `Tap` gate for the pull after its announcement, `NoDesk` or a `SheetDesk` for the router's sheets in an editor session, `fork` and `export` over the log, a stored session adopted again after a restart), `Tap` (how a turn is told as it runs and a call is held; `NoTap` is companiond's), restart (`restore`, `resume`; the roster and front come from `stored_events` over `Session.Stored` (`RouterLog::reading`) joined by `rebuild_from` with `recover` / `recent_events` over `RouterRecent` for messages, episodes and runs, which read a body bare or wrapped in the `Area` envelope memory answers with; a session an editor opened is left out; `Companion::without_notes` is the editor host's), the plan card, `Shared` (what a surface reads without waiting for the loop). No bus, no runtime, no clock read: companiond and `docket-inapp` each pass theirs | none (the transports, the clock and the surface are passed in) |
| `docket-session` | the durable session, portable and pure (design note `acp-sessions.md` sections 4, 5): `SessionEntry` (the append-only log of one session: opened, turn, policy as written, call, step, handle labels, write-ahead taint, breaker, budget checkpoint, skill, closed; `encode` / `decode` with an explicit version under the kind tags `companion.session.<slug>`), `resume_plan` (entries in, a `ResumePlan` out: taint never goes down, in-flight calls end interrupted and are never re-run, the policy as stored, handles as labels, a gap or an unreadable entry blocks and taints), `fork` (inherits taint and the breaker, resets budgets), `export` / `to_json` / `from_json`, `legacy` (the `companion_wire::SessionRecord`s companiond already writes, read as entries), and the seams `SessionLog` (append with a durable ack, page by session; docket-memory implements it over almanac; `sessions` lists the openings), `may_restore` / `may_restore_opening` (who may bring a stored session back: the opener rule, and for the cli role also a conversation whose `Opening::started_from` names a terminal), `SessionBackend` and `SessionHost` (traits; `fake` holds `FakeBackend` and `MemoryLog` for tests), `SheetDesk` (the router's open sheets an edge answers, handed out once per session by `next_sheet`; `NoDesk` holds none) and `contract` (the rules every backend keeps, run by each backend's test). No bus, no runtime, no clock read | none (the log is passed in) |
| `docket-acp` | the ACP server edge (design note `acp-sessions.md` section 3a): an editor drives the companion over the Agent Client Protocol on stdio. `Server` over a `Wire` (one JSON-RPC line each way), a `SessionHost` and a `SessionLog`: `initialize`, `session/new`, `session/load` (replays the stored turns and calls, then resumes), `session/list`, `session/prompt`, `session/cancel`, `session/set_mode` (modes `ask`, `read-only`, `auto-judged` shape only the editor's own prompt); anything else is method-not-found, `mcpServers` are ignored. `turn` is the pure step machine of one prompt (events and answers in, updates and orders out); `calls` maps step lines to tool calls (a refusal is `failed` with one coarse sentence; `rawInput` and `rawOutput` are never sent); `permission` is the extra gate at a call's start (`allow_once` and `reject_once` only; any other answer is a reject; an allow lets the call on to the router's gate and is no grant and no receipt) and the router's sheet put to the editor (`BackendEvent::Sheet`): `allow_always` is added only when the sheet's `always` is `Offered`, named by `scope_words` from the typed scope and the manifest's label, and its click reaches the host as `SheetChoice::Always` (`SessionHost::answer_sheet`; the host mints the receipt, the router records the grant); a withheld offer shows none and an unoffered `allow_always` is a refusal; `Covered` remembers the actions the person said "always" to on the connection so the extra gate stands down for them while the router's sheet still decides what the grant covers; `Permit` is the proof that `agent.acp.expose` is on. The wire types are `agent-client-protocol-schema` (Apache-2.0, 1.10.2, no schemars); the transport is ours. The client method side, `Terminals` (S8): `terminal/create`, `output`, `wait_for_exit`, `kill` and `release` over `bulkhead`, for a party that sends them to us (an external agent, S4); `create` runs only what the router allowed (it is `Execute`, gated as `Outbound`), `kill` and `release` are never gated, and `capabilities` turns the `terminal` client capability on only when the sandbox is there. Feature `server` (default) adds `LineWire` over tokio streams; the process is `docket-acp-bin` Feature `client` (S4 and its bridge to the router, off by default) is the other direction: `AcpBackend`, an external agent (Claude Code through its adapter, Gemini CLI) as a `SessionBackend` over a `Spawn` seam. We are the ACP client; we offer `fs/read_text_file`, `fs/write_text_file` and (only when the sandbox is there) `terminal/*`, and answer `session/request_permission` ourselves, from the router's ruling and never from the agent's own options. **The host decides nothing.** Each request is formed into an `AgentCall` (its path confined first: `confine` keeps `fs/*` inside the session's directory, lexically, then links via `Files::real`, then the descriptor again at the open in `OsFiles`; secrets refused even inside; a refusal never becomes a call and `Strikes` pauses the turn after five in a row) and made through the `Court` seam (`IntentsCourt` over `docket-client`, role `acp_agent`) as a router call of the `org.quire.AcpAgent` pseudo-app (`docket_core::agent_app`; declarations in `manifests/org.quire.AcpAgent.toml`, marked `visibility = "host_only"` so no planner, MCP client or ordinary session's policy writer is told of them: `acpagent.files.read`, `.files.write`, `.files.sensitive` (asks every time), `.terminal.run`, `.reported`, and `acpagent.<kind>` for permission kinds), so the policy point, reviewers, taint, breaker, budgets and audit apply, the actor is `acp:<program>`, standing grants are `GrantCaller::AcpAgent(program)` in docket's store and a permission the person allowed approves the matching call once. The router's `Perform` for the pseudo-app arrives at `Performer` (in a test through `HostedApp`, in the process through `IntentProvider1` on the host's bus name), which reads, writes or starts the command only for the held request (`StageId`) the call names; the text of a write and a command's argument vector never go to the router or a sheet. `AgentHost` is the `SessionHost`: it opens the agent's session at the router (`SessionOpen.external`), records the person's prompt there (`TurnSource::Agent`, so the task policy derives from it as for a launcher; the agent's own words are never a turn), and hands out the router's sheets (`BackendEvent::Sheet`, `answer_sheet`) only under `Fallback::Terminal` (`docket-agent --tty`); by default the sheet is the desktop's, through the router's confirmer. A file served taints the session (`TaintSource` says which one) and is remembered by the `Performer` (`Served`: its path and token-like strings) so a later command can be told as derived from it or not; what the agent only reports (`tool_call`) is shown as `acpagent.reported.<kind>` and, when it brought content in, told to the router so the taint is the router's; its thoughts are `Thought` events. `fake` holds an in-memory pipe, `FakeSpawn` and `FakeFiles` | `agent-client-protocol-schema`; tokio only behind `server` and `client` |
| `bulkhead` (external: its own repository, github.com/PoHsuanLai/bulkhead, pinned by rev; was `docket-shell`) | the sandboxed shell tool (S8), portable: the `Sandbox` and `Job` seam; `BwrapSandbox` (bubblewrap as a separate process, never linked: read-only root, `/home` `/root` `/run` `/tmp` and the like emptied, write only to the working directory, no network, cleared environment, own session, killed on drop) and `Detected` (the startup probe: bubblewrap, or the reason there is none); `FakeSandbox` (records the `RunSpec`s it is given, scripted output, shows kill and drop in `Seen`); `Shell`, the terminal table (it starts nothing the sandbox cannot confine: `ShellFault::CannotSandbox`); `redact`, `Tail`, `shown` (bounded, masked, valid UTF-8 output; a long output is a `Handle` for a planner via `view`); `sandbox_env` (allowlist, fixed `PATH` and `HOME`). The gate's rules for the `Execute` effect are in docket-core; bulkhead owns its `AbsPath`, `CannotSandbox` and `SandboxState`, and docket-acp (`bulk`) converts them to docket-core's. The confinement of a long-running agent process: `AgentRun` and `agent_bwrap_args` (the agent's environment is not in the arguments), `NetworkMode::{None, EndpointOnly, Host}` and `AgentNet`, and `forward` (the forwarder inside the sandbox and the `Bridge` outside, std only; the program `bulkhead::cli::forward_main`, which `docket-acp-bin` ships as the binary `docket-net-forward`, starts the agent behind the forwarder) | none (std processes only; no docket crate) |
| `docket-acp-bin` | the `docket-acp` process: the ACP edge on stdio hosting its own `Companion` (`docket-tasks`' `NativeHost`) over the session bus. It owns `org.quire.Acp`, which the shipped `intentd.toml` lists as an `editor` (opens sessions, records the editor's turns) and a `companion` (the planner's calls in them); inferd is the model, and stored sessions are read through `Session.Stored` (`RouterLog`), because memoryd answers log reads for the router and the shell only. It also serves `org.quire.Confirm1` under its own name (`confirm::ConfirmObject` over an `EditorDesk`): intentd hands it the sheets of calls in an editor's session (`ConfirmRequest.editor`) in place of sill, and only intentd's connection may ask or withdraw. `serve` is the lib's one entry, which the acceptance harness runs as `accept-acp`. The second binary, `docket-agent` (`agent`), is the host of an external coding agent: it owns `org.quire.AcpAgent` (listed under `acp_agent` in `intentd.toml`, a name only while `agent.acp.agents` is on), serves the pseudo-app's provider (`PerformerProvider` over the `Performer`), opens and records the agent's session through `IntentsCourt`, and with `--tty` also serves `Confirm1` for the sheets of its sessions | zbus, tokio (the bus and the runtime) |
| `docket-launch` | starts the external coding agents `docket-acp`'s client edge drives (S4): `AgentsFile` (`agents.toml`: program, command or a registry id with a pinned version, `model`, `sign_in`, route `endpoint` / `handoff` / `login`, network mode, read-only and read-write binds, plain variables, login command), `AgentsPermit` (`agent.acp.agents` on), `Accounts` (the seam to porter: register, sessions, inferd endpoints, grants, process credentials, revocations, login requests; `DbusAccounts` behind feature `dbus`, `FakeAccounts`), `AgentSpawn` (the real `Spawn`: a launcher session, the model route, the environment built from nothing, a bubblewrap process via `Procs`, and every duty undone on close), `Supervisor` (register, end the process of a revoked credential, run the agent's own login visibly through `LoginRunner`) | zbus and the bus behind `dbus`; tokio; bubblewrap as a separate process |
| `docket-agents` | coding agents from the agent registry; docket ships no list of its own: `Snapshot` (the registry's JSON, typed), `install` (a named version: download through the `Fetch` seam, SHA-256 checked against the registry's digest, unpacked under the agents directory it owns; a node package through the `Run` seam, a python package through `uv` in the same seam; the preference is binary, then node, then python), `standing` (what the list offers against what is installed, read only), `AgentsDir::launch` (the program, its arguments and the home it signs in inside, kept between runs), `Offered` (`state/<id>/offered.toml`: the models and sign-in ways the agent listed and whether the last start was `SignedIn`, `NeedsSignIn` or `Unknown`; written by `docket-agent` whenever a session with it opens, atomically, with a write counter instead of a clock). An update is `install` of the newer version the person names; nothing runs by itself. The `docket-agents` binary is the only caller that reaches the network, through `curl` | `sha2`, `flate2`, `tar`, `zip`; `curl` as a separate process |
| `docket-models` | the models docket asks, portable (moved out of intentd): `TransportModel<T: porter_client::Transport>` (a porter-infer `Model`: chat and embeddings, a session per request), `TransportWriter<T>` (the task-policy writer: the person's turns and the catalogue in, a checked `Derived` out: the `TaskPolicy` and any `Corrected` the checker made, such as a ceiling raised to its chosen actions' effect, audited as `PolicyCorrected`; `draft::policy_of` believes nothing it was not told), `reviewer_over` (the cascade of `action-review`'s `InferReviewer` from a set and one link per stage), `placeholder_set`, `card_of`, and `turn`, `chat_of`, `chat_need` that the reader shares. intentd's `InferdModel` and `InferdWriter` are thin adapters (the bus constructor) | per transport |
| `docket-reader` | the quarantined reader, portable (moved out of readerd): `reader_request` (the fixed instruction, the inputs fenced, no tools), `answer_of` (the reply read back under the ask's schema), `read` over a Transport and `TransportReader` (the in-process `Reader`: it opens the quarantined text itself, the session boundary standing in for readerd's process boundary). readerd re-exports the pure names and keeps `ReaderHost`, the bus and the Resolve calls | per transport |
| `docket-memory` | the memory seam, portable (moved out of intentd): `AlmanacMemory<T: almanac_client::Transport>` (the router's `MemoryLink`), `AlmanacSessionLog<T, K: Clock>` (`SessionLog` over `RecordDurable` and `Entries`), `record_of` / `kind_tag_of` (audit records as almanac events), `QueuedSink` (bounded), `AuditState` (drains a sink into any `MemoryLink`, Space by Space, with retry accounting, and reports what changed without printing: intentd's `AuditLog` prints, the in-app agent does not). The transport is the caller's: memoryd over D-Bus, or almanac's `in_process` service | per transport |
| `docket-inapp` | the in-app agent (design/36): `InAppAgent` hosts the router, policy point, reviewer and `docket-tasks`' `Companion` (the same task model companiond runs: many tasks, the front pointer, the roster, side conversations, the idle `tick`, restart `restore`, `new_task`, `ask_in`, `told`) inside one app over that app's own `IntentProvider`, with skills (`install_skills`, `install_skills_from`: the `docket-skills` format; text teaches and grants nothing) and a durable audit queue (`AuditFile`: records waiting for memory kept in a file the app names, atomic writes, bounded, typed `AuditFileError`, queued again by the next agent), with `EditorDesk` (a `ConfirmSheet` whose open sheets wait for an editor's choice, and a `SheetDesk` the session host reads them from), `ConfirmSheet` (the app's own sheet; `SheetConfirmer` mints the receipt), `ProviderLink`, `InAppSeams` and the stubs `SessionGrants`, `NoMemory`, `NoReader`, `NoWriter`. `InAppKit` swaps in the real parts: `FileGrantStore` (consent in a file the app names: atomic writes, typed `GrantFileError`, intentd's file format), `AlmanacMemory` (recall, primer, profile and episodes in the planner's view, and under `AuditTo::Memory` the turn's audit and the task's episode written at the end of each turn), `TransportWriter`, `TransportReader`; `reviewer_over` builds the model-backed reviewer for `InAppParts`; `SystemClock` is the router's `Clock` with no runtime (one timer thread, std only). `AuditBuffer` is a `QueuedSink`. No intentd, no bus, no daemon | none (the transports, the sheet and the grants file's path are passed in) |
| `docket-dbus` | `org.quire.Intents1` (ten interfaces), `IntentProvider1`, `Confirm1`, `Companion1` (+ `.Answer`) and `Reader1` as zbus proxies and skeletons, `introspection`, `IntentsError`, bus names and paths, `session_connection` (the session bus of a daemon, from its environment); with feature `inferd`, `inferd_transport`, the one constructor of every daemon's inferd link (an `InferLink`: the transport behind `tap`, the model tap a live run turns on with `DOCKET_MODEL_TRACE`, honoured only in a build with feature `test-model-trace`, which only docket-accept enables) | zbus |
| `docket-client` | the app side (`IntentProvider`, `ContextSource`, `SummonTarget`, `serve`, `serve_on`: `IntentProvider1` on the app's own name, answering intentd alone) and the caller side (`Intents` over a `Transport`: `InProcess`, `DbusTransport` behind feature `dbus` (on by default for a consumer outside the workspace; the workspace declares the crate with `default-features = false` and each desktop crate names `dbus`), whose `connect` finds or activates intentd and whose `call` carries all 36 members, and `Transport::watch` / `Intents::gate_check_watched` (a gate check whose `Progress(Confirming)` is heard and whose `Proceed` and `Close` are said) and `Intents::perform_watched` (a `Run.Perform` whose `Progress` is heard: `Reviewing`, `Previewing`, `Confirming(id)`, `Dispatched`; nothing waits for the caller); `requested` waits for a Request object's `Response`) | per transport |
| `docket-fake` | test only: fixture manifests, `FakeMail`, `FakeFiles`, `ScriptedConfirmer`, `ScriptedReviewer`, `ScriptedWriter`, `ParsedReviewer` (a reviewer whose replies are raw text read by `parse_verdict`), `ScriptedReader`, `FakeMemory`, `FixedClock` (a virtual clock: `advance`, `asked`, `next_ask`), `RecordingSink`, `MemoryGrants`, `FakeSeams`, `fake_router` | none |
| `docket-testbus` | test only: `PrivateBus` (a private session bus from a scratch config; the daemon is killed by PID, and waited for, when it drops, and by a watchdog if the test process is killed) and `Reaped`, the guard under it; every daemon's bus tests use it | zbus |
| `docket-accept` | test only: the agent tier's end-to-end acceptance. Thin mains run intentd, companiond and readerd as processes (`accept-*`); `build.rs` builds porter's `inferd` and almanac's `memoryd` (feature `test-keys`) from their own workspaces into `<target>/accept-siblings`, and inferd plays the cassettes in `dev/accept/cassettes`; the library holds the `Confirm1` server, the mail provider, the world (private bus, scratch dirs, event-driven waits) and the launcher's calls; the live harness (`live/`) also warms every routed model before a live run (`warm.rs` the pure plan and wait, `warm_bus.rs` porter-client's `Transport::prepare`). `dev/accept/` holds the fixtures, README and `run.sh` | intentd, companiond, readerd, docket-testbus (inferd and memoryd by build.rs) |
| `docket-eval` | the red-team suite: the corpus format and loader (`eval/`), `Case` (`Driver`: the companion or a terminal), `Expect`, `RunReport`, `Metrics`, `wilson`, the runner skeleton and `Harness`; the hostile-model corpus (`ModelScript`: what the writer and reviewers say, raw; `PlannerCase`: the companion over a hostile cassette); and the conformance check `docket-eval --check-app <dir>` (`check`, `ui`) | none |
| `docket-cli` | `quire-do`: every app drivable from a command line generated from its manifest. A thin client over `docket-client` with caller role `cli`: the command grammar, the parameter mapping, `describe` through stoker's `Shape::to_json_schema`, the exit codes, text and JSON output, shell completion, and `ask`: the person at a terminal talks to the companion (`Open`, `Session.Turn` recorded as `TurnSource::Terminal`, `Ask`, the answer followed; a confirmation is printed, never answered: exit 8), and `sessions` (list, load and fork the stored sessions the terminal may bring back, through `Session.Stored`; the router writes a fork). It reaches the bus only through `docket-client`, and never the router, Cedar or the policy point | per transport (the binary: the session bus) |
| `actions-tools` | the tools an outside agent is offered, pure: `tools`, `offered`, `find`, `tool_of`, `tool_name`, `hints_of`, `mcp_label`, `read_call`, `outcome_json`, `McpFault`, and the lines of the per-session edge (`EdgeRequest`, `EdgeReply`, `EdgeToken`). actions-mcp re-exports them; docket-acp's per-session edge serves the same | none |
| `actions-mcp` | the MCP edge: `tools`, `tool_name`, `hints_of`, `mcp_label` (from actions-tools), `McpExpose` (the setting `agent.mcp.expose`, off by default; `settings.rs` reads `docket/settings.toml` through `docket-settings`, again at every request, because this crate may not link a watcher), `read_call` (docket-core's `args_from_json`), `McpFault`, `McpEdge` over `rmcp`, which serves `list_tools` and `call_tool`; the binary (`McpConfig`, `claim`, `start`, `run`): stdio or `--socket`, off unless the settings file says `agent.mcp.expose = "on"`, one bus name (`org.quire.ActionsMcp`, the `mcp` role); `--host-socket PATH` is instead the bridge an external ACP agent starts (`BoundEdge`: no bus, the session's token in `QUIRE_EDGE_TOKEN`) | rmcp |
| `docket-settings` | the person's settings (design/22 section 3.27): `read` (text in, `AgentSettings` out: lenient, a bad value falls back per key and is a `Fallback`), the key table (`keys.rs`, held to the schema by tests), `Locator` (`$XDG_CONFIG_HOME/docket/settings.toml`, then `$XDG_CONFIG_DIRS`), `McpExpose`, `SCHEMA` (`dist/settings/docket.settings.toml`), `REVIEW_CEILING`, and `places/`: the "where the assistant may run" setting (`Places`: this computer, `OwnComputer`s, `CloudAccount`s; `Toggle` / `CloudAllowed` so only cloud can be "ask each time"), `read_places` / `write_places` (table `assistant`, a bad value is Off), `Places::merged` / `migrated` (a place with no row is Off), `route` (floor filter, then this computer, your computers, cloud: `Chosen` / `Ask` / `NoAllowedPlace` / `Blocked`), the plain-words strings, and the `PlaceSource` seam inferd fills; and `agents/`: `read_agents` (the `agents.toml` text and the agents directory in, one `AgentRow` per entry out: label, `Source` / `Install`, `ModelState` with `Availability`, `Offers`, `SignInState`; plain-words lines), `write_agent_choice` / `edit_agent_choice` (the model and sign-in way of one entry, edited in place with `toml_edit`, comments kept) and `RefreshRequest` (the arguments of `docket-agent PROGRAM --refresh`; there is no settings bus). No bus, no runtime, no watcher: intentd owns the directory watch | the settings file |
| `intentd` | the daemon and its library: `IntentdConfig` (and the shipped `dist/intentd.toml`), the built-in `org.quire.Memory` and `org.quire.Companion` providers and `HostedLink` (which answers them in process), `AlmanacMemory`, `QueuedSink` (bounded), `record_of` (all three re-exported from `docket-memory`) and `AuditLog` (the audit trail into memoryd: `docket-memory`'s `AuditState` and the log lines), `DbusLink`, `SheetConfirmer`, `FileGrants`, `InferdModel`, `InferdWriter` (adapters over `docket-models`), `ReaderClient`, `SystemSeams`, `Peers` (who is on a connection: names, else the cgroup read through porter's `ProcCallers`; `ProcRoot`, the test-only `INTENTD_PROC_ROOT`; the terminal scope names are `docket_core::is_terminal_scope`), `serve` / `serve_on` (one handler per `Intents1` interface), `signals` (`Marks`, `changes`, `pump`: the signals that say the router's state changed), `SettingsWatch` (the directory watch on `docket/settings.toml`; `apply_next` puts each change in force on the router through `Router::apply_settings`), `watch_logind` (the end of the person's session), `start` / `run` (the daemon) | everything |
| `companiond` | the companion daemon, the bus adapter over `docket-tasks`: `Companiond` (`docket_tasks::Companion` over the system `Clock` and the `Bell`, the surface that fans changes out on the bus: a broadcast channel, a `Notify`, an answer's object path), `follow` (a pressed card's call read to its end on the bus), `PlannerModel` (docket-planner's, over inferd) over a `Catalogue`, `Shared` (what the bus reads), `recover` (and `RouterRecent`, its `RecentSource` over the router), `completion_effects` (all re-exported from docket-tasks so the names and the tests are unchanged), `CompaniondConfig`, `start` / `run` (the daemon), `serve`, `serve_on` and `serve_on_rooted`, `speaker` (who speaks for the person: the shell for everything, a terminal, carrying its `TerminalScope`, for `Open`, `Ask` and `Close` only; `Open` writes that scope into `SessionOpen::started_from` so the router records which terminal a `quire-do ask` came from; the test-only `COMPANIOND_PROC_ROOT`) | everything |
| `readerd` | the quarantined reader, a separate process: `ReaderHost` (the one place the reader key is made on the desktop), `ReaderService` (resolves handles over `Intents1`, reads with `docket_reader::read`), `serve`, `start` / `run` (the daemon) | everything |
| `docket-ds` | the adapter quire apps use: chips and keep, things and labels, summon answers, `DsContextSource`, `DsSummonTarget`, the voice bridge | none |
| `voice-wire` | the bodies of `org.quire.Voice1`: `VoiceBegin`, `VoiceEvent`, `UtteranceEnd`, `VoiceStatus`, `VoiceRefusal` and its 1:1 error names, `SpeakWire` | none |
| `voice-loop` | the voice machines: `utterance_step` (the microphone is open exactly in Opening, Listening and Tail), `speech_step` (barge-in), `sentences`, `PcmBuffer` | none |
| `voiced` | the microphone's owner: `VoicedConfig`, the `AudioDevice` seam and `choose_capture` (never a monitor), the `Voice1` interfaces and their introspection, `serve` (roles from the connection, one loop over the two `voice-loop` machines, inferd through porter-client), the PipeWire device | everything |

Allowed direct edges (checked by `scripts/check-boundary.sh`; dev-dependencies are outside it):

| Crate | May depend on |
| --- | --- |
| `docket-core` | `prov`, `porter-core`, `almanac-core`, `cua-action`, `model-provider` |
| `docket-skills` | `docket-core`, `prov`, `porter-core`, `toml` |
| `policy-point` | `docket-core`, `prov`, `porter-core` |
| `action-review` | `docket-core`, `prov`, `porter-core`, `porter-infer`, `model-provider` |
| `docket-router` | `docket-core`, `docket-session`, `docket-skills`, `policy-point`, `action-review`, `prov`, `porter-core`, `almanac-core` |
| `companion-wire` | `docket-core`, `prov`, `porter-core`, `porter-infer`, `almanac-core` |
| `companion-client` | `companion-wire`, `docket-core`, `docket-client`, `docket-dbus`, `prov` |
| `agent-loop` | `docket-core`, `companion-wire`, `almanac-core`, `porter-core`, `prov` |
| `docket-planner` | `agent-loop`, `almanac-core`, `companion-wire`, `docket-core`, `porter-client` (no features), `porter-core`, `porter-infer`, `prov` |
| `docket-tasks` | `agent-loop`, `almanac-core`, `companion-wire`, `docket-client` (no features), `docket-core`, `docket-planner`, `docket-session`, `docket-skills`, `porter-client` (no features), `porter-core`, `porter-infer`, `prov` |
| `docket-session` | `companion-wire`, `docket-core`, `porter-core`, `prov` |
| `actions-tools` | `docket-core`, `porter-core`, `prov` |
| `docket-acp` | `actions-tools` (feature `client`), `companion-wire`, `docket-client` (no default features, feature `client`), `docket-core`, `docket-session`, `docket-settings`, `bulkhead`, `porter-core`, `prov` (+ `agent-client-protocol-schema`) |
| `docket-launch` | `docket-acp` (features `client`, `server`), `docket-agents`, `docket-core`, `docket-session`, `docket-settings`, `bulkhead`, `porter-core`, `prov`; `porter-client` (feature `dbus`) and `porter-dbus` behind its `dbus` feature |
| `docket-agents` | `docket-core` (for the atomic file write) (`serde`, `serde_json`, `toml`, `thiserror`, `sha2`, `flate2`, `tar`, `zip`) |
| `docket-acp-bin` | `docket-acp` (feature `client`), `docket-agents`, `docket-client` (feature `dbus`), `docket-core`, `docket-dbus` (feature `inferd`), `docket-inapp`, `docket-launch` (feature `dbus`), `docket-planner`, `docket-router`, `docket-session`, `docket-settings`, `bulkhead`, `docket-tasks`, `porter-core`, `prov` |
| `docket-models` | `action-review`, `docket-core`, `porter-client` (no features), `porter-core`, `porter-infer`, `prov` |
| `docket-reader` | `docket-core`, `docket-models`, `porter-client` (no features), `porter-core`, `porter-infer`, `prov` |
| `docket-memory` | `almanac-client` (no features), `almanac-core`, `docket-core`, `docket-router`, `docket-session`, `porter-core`, `prov` |
| `docket-inapp` | `action-review`, `agent-loop`, `almanac-core`, `companion-wire`, `docket-client` (feature `in_process`), `docket-core`, `docket-memory`, `docket-models`, `docket-planner`, `docket-reader`, `docket-router`, `docket-session`, `docket-skills`, `docket-tasks`, `policy-point`, `porter-client` (no features), `porter-core`, `prov` |
| `docket-dbus` | `docket-core`, `prov`, `porter-dbus`; `porter-client` with feature `inferd` |
| `docket-client` | `docket-core`, `docket-router`, `prov`; `docket-dbus` with feature `dbus` |
| `docket-fake` | `docket-core`, `docket-router`, `docket-session`, `docket-client`, `policy-point`, `action-review`, `prov`, `porter-core`, `almanac-core` |
| `docket-testbus` | `docket-dbus` |
| `docket-accept` | `actions-mcp`, `almanac-client`, `almanac-core`, `companion-wire`, `companiond`, `docket-acp`, `docket-acp-bin`, `docket-agents`, `docket-cli`, `docket-client`, `docket-core`, `docket-dbus`, `docket-inapp`, `docket-launch`, `docket-router`, `docket-session`, `docket-settings`, `bulkhead`, `docket-testbus`, `intentd`, `porter-core`, `porter-infer`, `prov`, `readerd` |
| `docket-eval` | `action-review`, `docket-core`, `docket-skills`, `docket-fake`, `docket-router`, `prov`, `porter-core` |
| `docket-settings` | `docket-agents`, `docket-core`, `porter-core` (`toml_edit`, `thiserror`) |
| `actions-mcp` | `actions-tools`, `docket-core`, `docket-client`, `docket-dbus`, `docket-settings`, `prov`, `porter-core` (+ `rmcp`) |
| `intentd` | `docket-core`, `docket-memory`, `docket-session`, `docket-models`, `docket-skills`, `docket-settings`, `docket-router`, `docket-client`, `docket-dbus`, `policy-point`, `action-review`, `prov`, `porter-core`, `porter-infer`, `porter-client`, `almanac-core`, `almanac-client` |
| `companiond` | `companion-wire`, `docket-core`, `docket-planner`, `docket-skills`, `docket-tasks`, `docket-settings`, `docket-client`, `docket-dbus`, `prov`, `porter-client`, `porter-core` |
| `readerd` | `docket-core`, `docket-client`, `docket-dbus`, `docket-reader`, `prov`, `porter-client`, `porter-core`, `porter-infer` |
| `docket-cli` | `companion-client`, `companion-wire`, `docket-core`, `docket-session`, `docket-skills`, `docket-client` (feature `dbus`), `model-provider`, `prov`, `porter-core` |
| `docket-ds` | `docket-core`, `docket-client`, `companion-wire`, `voice-wire`, `prov`, `porter-core`, `ds-intents` |
| `voice-wire` | `docket-core`, `porter-core`, `porter-infer` |
| `voice-loop` | `voice-wire`, `docket-core`, `porter-core`, `porter-infer` |
| `voiced` | `voice-loop`, `voice-wire`, `docket-core`, `porter-core`, `porter-dbus`, `porter-infer`, `porter-client`, `speech-provider`, `speech-vad` |

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

## 1a. Portable core and desktop extras

The rule (quire design/36): everything except the desktop environment is cross-platform, and
the desktop's features are additive extras that are probed at run time and absent from foreign
builds. For docket that makes two sets.

**Portable core: an in-app agent.** These crates build with `--no-default-features` on macOS and
Windows, reach no `zbus`, `inotify`, `landlock`, `pipewire` or `/proc`, and carry no D-Bus type
in a public signature: `docket-core`, `docket-skills`, `policy-point`, `action-review`,
`docket-router`, `companion-wire`, `agent-loop`, `docket-planner`, `docket-tasks` (the companion's task model over any transports, clock and surface), `docket-session` (the durable session record and its resume rules), `docket-client` (the app side,
`IntentProvider`, and `InProcess`, without its `dbus` feature), `docket-fake`, `docket-eval`,
`docket-models` (the model-backed reviewer and policy writer over any transport), `docket-reader` (the quarantined reader over any transport), `docket-memory` (the memory seam over any almanac-client transport), `docket-inapp` (the host that runs the others inside one app), `voice-wire` and `voice-loop`.
An app that embeds `docket-inapp` gets the whole gate (Cedar policy point, reviewer, breaker,
budgets, the confirm sheet only the person can answer) with no other process.

**Desktop extras: one companion across every app.** `docket-dbus` and `docket-client`'s `dbus`
feature (the transport), `intentd` (the cross-app broker and its audit trail), `companiond` (one
identity over many tasks), `readerd` (the quarantined reader as a process around `docket-reader`), `voiced` (the
microphone), `actions-mcp`, `quire-do` (`docket-cli`), `companion-client`, `docket-settings` (the
settings file the shell's pages write), `docket-ds` (quire's adapter), `docket-testbus` and
`docket-accept`. cua (computer use) is above docket and desktop only. These light up when
`org.quire.Intents1` answers; a missing one means the in-app path, never an error.

Two kinds of Cargo feature, kept apart. A *platform feature* says what the OS provides (inotify,
PipeWire, `/proc`): named for the thing, off where the OS lacks it. The *app-level desktop switch*
is `quire-desktop`, mapped by implication: a crate with a D-Bus client has
`quire-desktop = ["dbus"]` (default on), and `dbus` stays the transport feature that pulls zbus.
`docket-client` is the one such crate here (`docket-dbus` is the D-Bus layer itself and has no
switch; its `inferd` feature names a transport, not the desktop). A portable build is
`--no-default-features`.

`scripts/check-portable.sh` is the mechanical form: `cargo check --no-default-features` on exactly
the portable list, a `cargo tree` grep for the forbidden crates, and `cargo check --target` for
every cross target rustup already has. `scripts/check-boundary.sh` holds the rows (the portable
rows never list a bus). The seams that have no portable answer yet are the "what is still
missing" list in `FINDINGS.md` (portable-core).

## 2. Modules

| Crate | Modules |
| --- | --- |
| `docket-core` | `units`, `ids` < `value`, `args`, `manifest` < `validate`, `schema` < `context`, `preview` < `call`, `undo`, `grant`, `confirm`, `review`, `budget` < `task_policy`, `reader`, `planner`, `roster`, `skill` < `message`, `task`, `audit`, `gate`, `index`, `summon` < `config`, `caller`, `wire`, `when` |
| `docket-skills` | `fault`, `skill` < `discover`, `library` |
| `policy-point` | `request` < `pdp` |
| `action-review` | `verdict` < `request`, `breaker` < `cascade`, `infer` |
| `docket-router` | `auth`, `registry`, `session`, `index`, `journal`, `handles`, `messages`, `tasks`, `convert`, `wal` < `gate`, `call` < `seams` < `state`, `labels`, `argcheck`, `consent`, `coverage`, `who`, `companion`, `skills` < `prepared`, `prepare`, `driven`, `confirm`, `perform`, `finish` < `policy`, `terminal`, `dryrun`, `opening`, `reading`, `messaging`, `search`, `control`, `gatecheck`, `watch`, `notes`, `recall` < `recording`, `durable`, `rebuild` < `restore` < `router` |
| `companion-wire` | `ask`, `answer` < `record` |
| `companion-client` | `lib` (`CompanionTransport`, `Follow`) < `bus` (`DbusCompanion`, `BusAnswer`) |
| `agent-loop` | `tier`, `front`, `completion`, `side`, `idle`, `rebuild`, `assemble` < `step` |
| `docket-planner` | `args`, `catalogue`, `read_ask` < `render` < `planner` |
| `docket-models` | `model`, `draft` < `writer`, `reviewers` |
| `docket-reader` | `request`, `answer` < `read` |
| `docket-memory` | `memory`, `record`, `session_log` < `sink` < `audit` |
| `docket-inapp` | `link`, `sheet`, `seams`, `grants`, `clock` < `kit` < `turn` < `agent` < `recall`, `drive` |
| `docket-dbus` | `names`, `error`, one file per interface, `introspect` |
| `docket-client` | `provider`, `transport` < `watch` < `awaiting`, `watch_bus`, `watch_in_process` < `bus` < `intents`, `session_calls`, `provider_bus` < `serve` |
| `docket-fake` | `labels`, `simple`, `mail`, `files`, `scripted`, `seams`, `router` |
| `docket-testbus` | `guard` < `lib` (`PrivateBus`) |
| `docket-settings` | `expose`, `keys` < `read` < `locate`, `places`, `agents` (`tests`: the schema against the table) |
| `docket-accept` | `provider`, `confirm` < `world` < `drive` |
| `docket-eval` | `case`, `report`, `corpus`, `block`, `world`, `steps`, `runner`, `metrics`, `shadow`, `check` < `ui` (the binary `docket-eval` runs `check`) |
| `docket-agents` | `slug`, `platform`, `hash`, `snapshot`, `dirs`, `record`, `offered`, `fetch`, `run`, `unpack`, `uv` < `install`, `launch` |
| `docket-launch` | `config`, `accounts`, `names`, `permit`, `env`, `procs`, `fake`, `login` < `spawn` < `supervise`, `dbus` (feature) |
| `docket-session` | `entry`, `plan` < `codec`, `legacy` < `resume` < `fork`, `export`, `log`, `backend`, `desk` < `fake`, `contract` |
| `docket-acp` (`client`) | `names`, `taint`, `strikes`, `confine` < `files`, `court` < `tool_req`, `performer`, `reported`, `rpc`, `intake` < `backend` < `handlers` < `serve`, `host`, `intents_court`, `spawn`, `fake` |
| `docket-cli` | `exit`, `args` < `resolve`, `when` < `params`, `schema`, `outcome` < `render`, `complete`, `help` < `exec`, `ask_render` < `ask` (`quire-do ask`) < `lib` (`run`), `program` (the whole program; `main` and the acceptance harness's `accept-quire-do` call it), `main` |
| `intentd` | `config`, `builtin_memory`, `builtin_companion` < `builtin`, `audit`, `defaults` < `grants`, `procroot` < `peer` < `sheet`, `link`, `infer`, `writer`, `reader_client`, `reviewers`, `system`, `settings_watch`, `bus` (`request`, `query`, `run`, `session`, `control`) < `serve`, `logout`, `manifests` < `signals` < `daemon` (`main`) |
| `docket-tasks` | `seams`, `fault`, `shared`, `plan`, `linger` < `task` < `runtime` < `completion`, `recover`, `sources`, `drive`, `act`, `inbox`, `finish`, `idle`, `records`, `held`, `resume`, `tap` < `native` (`end`, `flight` < `backend`, `stored`, `log` < `host`) |
| `companiond` | `clock`, `bell` < `follow`, `serve`, `config` < `daemon` |
| `readerd` | `host` < `service` < `serve` < `daemon` (`main`) |
| `docket-ds` | `chips`, `things`, `summon`, `context`, `voice` |
| `voice-wire` | `text`, `begin`, `event`, `status`, `refusal` |
| `voice-loop` | `buffer`, `sentencer`, `utterance`, `speech`, `coordinate` |
| `voiced` | `names`, `config`, `device`, `bus`, `introspect`, `serve`, `engine`, `utter`, `hear`, `talk`, `command`, `peer`, `link`, `playback`, `sink`, `usage`, `warm`, `wire`, `error`, `pipewire_device` |

## 3. One home per concept

| Concept | Home |
| --- | --- |
| who acted, effects, labels, ids of sessions, runs, tasks, messages and entities, the confirmation receipt, `Message`, `AgentRef`, `Address` | porter `prov` (never redefined here) |
| the manifest and `validate`, `ActionDecl`, `ParamDecl`, `ArgSink`, `Value`, `Args` | `docket-core::{manifest, validate, value}` |
| the person's `agent.*` settings: the file, the key table, the schema, the lenient reader | `docket-settings` (one table; the schema `dist/settings/docket.settings.toml` and the table are held to each other by its tests). The values reach the router as an `AgentConfig` (`Router::apply_settings`, read afresh by every decision) |
| the four rulings of policy (`Ruling`), the reasons to ask (`AskReason`), the coarse `DenyCode`, `Stage`, `Impact`, `ReasonCode`, `BreakerTrip`, `Strictness` | `docket-core::review`: the refusals and the audit name them, so they sit below the crates that decide |
| the reviewer's `ReviewVerdict`, `tighten`, `plan`, the denial `Breaker` | `action-review` |
| the Cedar schema, default policies, `Pdp` | `policy-point`, files in `policy/` |
| the terminal's rows of the grid (`principal.kind == "cli"`) and its standing grant | `policy/default.cedar` (`cli-asks`, `cli-destructive-asks`, `cli-granted-final`); the grant itself is `docket-router::terminal`: a task policy of the terminal's session naming the action, given only by `ConfirmAnswer::AllowedFromTerminal` |
| the `Execute` rules (S8), now the router's own (the policy point and `may_offer`, no separate function): confirm by default; a terminal-scoped standing grant stands in only for a sandboxed command; a command that cannot be sandboxed, one that can send data out (`NetReach::Possible`: a network tool, or any network in the command's sandbox or the agent process's own, R12: `Performer::with_agent_network`, `NetAccess::combine`), one whose arguments derive from what the agent read (`Served::derivation`; any command, if the host could not see what the agent took in), a tripped breaker and an exhausted budget ask with no "always", while the session having merely read something does not (R11; the host sends `derives` and `network` as parameters of `acpagent.terminal.run`); a reviewer can only tighten. prov's `Effect` has no `Execute` (porter's frozen enum), so `EXECUTE_AS` gates it as `Outbound`. The router rules an agent's command (`acpagent.terminal.run`, with `command` and `cwd` parameters); `exec_reach` (`NetReach`, `NetAccess`), `exec_derive` (`Served`, `Derivation`) and `exec_facts` (`ExecFacts`) are the pure R11 rules; `EXECUTE_AS`, `SandboxState` and `CannotSandbox` are what is left in `execute` (`rule_execute` and its facts were deleted once nothing called them) | `docket-core::execute` (pure); the commands run by `docket-acp::Terminals` after the router allowed them |
| standing grants ("allow always" for an editor or an ACP agent): the key (`StandingScope`: files = action + path prefix, terminal = command prefix + cwd, outbound = action + recipient or domain; `GrantCaller::{Editor, AcpAgent}`), the match, the never-grantable rule and the offer (`may_offer`, `blocker`), the list codec | `docket-core::{standing, standing_match, standing_offer}` (pure). Held in the consent store (`GrantStore::standing`, `add_standing`, `revoke_standing`: `FileGrants` in `standing.json` beside `grants.json`, `FileGrantStore` in `<file>.standing`), read afresh on every call. `Who::grant_caller` names an editor's calls `GrantCaller::Editor(ClientName)` from the app behind the editor's connection (`Who::editor`: the app of the session's latest `TurnSource::Editor` turn, or the connection itself for the `Editor` role); a later launcher turn in the session ends it. An editor's typed turn derives the task policy as a launcher's does (only a prompt field's turn is capped to its app plus reads) The router reads the facts (`standing_facts`), lifts the ask (`standing::StandingCtx::lifted`), audits (`StandingGranted`, `StandingUsed`, `StandingRevoked`) and serves `.Control.StandingGrants` / `.RevokeStandingGrant` (Control role) |
| `quire-do`: the command grammar, the parameter mapping, the exit codes | `docket-cli` (`args`, `params`, `exit`); a terminal's arguments are labelled `Untrusted, Source::Cli` by the router, never by the client |
| the conformance check of every app repo | `docket-eval --check-app` and `scripts/check-intents.sh` (cli.md section 6) |
| the planner's view and its builders: the view type | `docket-core::planner`; the one builder, `planner_view`, and the `HandleTable`, `docket-router::handles` |
| a past step as the planner reads it (`StepLine`: `with` the handles named, `end` with the handles returned; masked to `action #1 → #4 [outcome: ...]`, at most four handles per place) | `docket-core::planner`; the masking, `agent-loop::assemble::mask_history` (keeps only handle values); the text, `docket-planner::step_text` (pure: same step, same bytes). Reviewer stages never ask for reasoning (`action-review::infer::control`); a reply that was only thought is `ReviewError::OnlyThought` |
| the roster and episode lines the planner reads | `docket-core::roster` (the view holds them) |
| episodes and their skeleton | almanac `Episode`; docket builds the skeleton (`docket-core::task::skeleton_of`) and the router records it |
| the one message model | porter `prov::Message`; docket's `MessageDraft`, `Delivery`, `InboundLine` are input and views |
| the task policy, `compare`, `covers`, `PolicyWriter`; `paths` bound the files a call targets (whole components, the standing-grant path semantics; an empty list leaves files unbounded; a target outside is `OutsideTask`; dropping every path is a widening; the writer's relative paths are anchored to the session's directory by the router) | `docket-core::task_policy`, `docket-router::policy` |
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
| `AppLink`, `GrantStore`, `EventSink`, `Clock`, `MemoryLink` | `docket-router` | `HostedLink` (over `DbusLink`), `FileGrants`, `FileGrantStore` (docket-inapp, portable), `QueuedSink`, `SystemClock` (intentd: tokio; docket-inapp: std only), `AlmanacMemory` (docket-memory); the fakes |
| `Confirmer` | `docket-core` | `SheetConfirmer`, `ScriptedConfirmer` |
| `Reviewer` | `action-review` | `InferReviewer` (over `TransportModel`, built by `reviewer_over`), `ScriptedReviewer` |
| `PolicyWriter` | `docket-core` | `TransportWriter` (docket-models; `InferdWriter` adapts it), `ScriptedWriter` |
| `Reader` | `docket-core` | `TransportReader` (docket-reader, in process), `ReaderService` (readerd), `ReaderClient` (intentd's end of `Reader1`: it names the session, readerd resolves the handles through it), `ScriptedReader` |
| `IntentProvider`, `ContextSource`, `SummonTarget` | `docket-client` | apps; `DsContextSource`, `DsSummonTarget`; the built-in providers; `FakeMail`, `FakeFiles` |
| `Transport` | `docket-client` | `InProcess`, `DbusTransport` |
| `Now`, `Surface` | `docket-tasks` | `Clock` and `Bell` (companiond), `HostClock` and `Quiet` (docket-inapp), a test's hand |
| `SessionLog` (a member of `Seams`: `type Log`) | `docket-session` | `MemoryLog` (fake), `AlmanacSessionLog` (docket-memory, over almanac's `RecordDurable` and `Entries`), `NoLog` (docket-router: an app with no memory), `Arc<L>` |
| `SessionBackend`, `SessionHost` | `docket-session` | `FakeBackend`, `FakeHost` (fake_host: scripted backends over a shared `MemoryLog`; records the sheet choices it is given); `NativeBackend`, `NativeHost` (docket-tasks, the planner loop of a `Companion`); `AcpBackend`, `AgentHost` (docket-acp `client`, an external agent); `docket-acp` consumes `SessionHost` |
| `Spawn`, `AgentChild` | `docket-acp` (`client`) | `AgentSpawn` (docket-launch), `FakeSpawn` |
| `Court`, `Files` | `docket-acp` (`client`) | `IntentsCourt` (over any `docket-client` transport), `OsFiles`, `FakeFiles`; the sheet is the router's confirmer's (sill's), or `docket-agent --tty`'s terminal prompt |
| `Accounts`, `Procs`, `LoginRunner` | `docket-launch` | `DbusAccounts`, `BwrapProcs`, `VisibleLogin`; `FakeAccounts`, `FakeProcs` |
| `RecentSource` | `docket-tasks` | `RouterRecent` (`Session.Recall` through intentd's router: a body only for a trusted entry), a fake |
| `PromptHost`, `DictationBridge`, `WindowFacts` | `docket-ds` | quire apps |
| `AudioDevice` | `voiced` | `PipeWireDevice`, `FakeAudioDevice` (feature `testing`) |

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
                                    unless a standing grant covers the call and `blocker` finds nothing
                                    never-grantable (untrusted into a sink, outside the task, breaker,
                                    budget, destructive): then the reviewer cascade below runs in its
                                    place, the grant skips only the ask, and its use is audited
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
`Registry::unresolved` check it in `tests/it/manifests.rs`; add its behaviour to the provider:
`intentd::builtin_memory` for `org.quire.Memory`, `Router::companion_perform` (docket-router,
`companion.rs`) for `org.quire.Companion`. `HostedLink` routes the two names to them.

**Add a member to `Intents1`.** (1) A variant of `IntentsRequest` and its reply in
`docket-core::wire`; (2) `Member` and `IntentsRequest::member`; (3) its row in `docket-router::auth`
(who may call it, and a test that names the guarantee); (4) the method in `docket-dbus`'s table
for its interface; run `cargo test -p docket-dbus`, which prints the new XML; replace
`dbus/org.quire.Intents1.xml` with it in the same commit; (5) a method on `docket_client::Intents`.

**Add a Cedar rule.** Write it in `policy/default.cedar` with an `@id`; `Pdp::load` validates it
strictly against `policy/quire.cedarschema`; add the row to `policy-point/tests/it/grid.rs`. A rule
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
- Cross-repo dependencies are git dependencies at pinned revs (porter, almanac, stoker, quire), so a plain
  clone builds with no sibling checkout. The pins use the same URL and rev as the other repos' pins of the
  same crates, or cargo links two copies. Local cross-repo work overrides a pin with a `[patch]` (FINDINGS.md).
  External versions are copied from quire's `docs/workspace-deps.toml`, never chosen here.
- Behaviour is `todo!()` behind frozen signatures, each listed in `FINDINGS.md`; shape tests
  (round trips, pinned JSON, introspection, tables) pass.
- Tests never touch the real system: no real bus, no real apps, no GPU, no real engine, no network.
  intentd, companiond, readerd and voiced serve; the first three have a binary test on a private bus, and voiced's `start` is tested on one over a scripted device (the real PipeWire device is `dev/voice-capture-try.sh`, by hand).
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
  links libpipewire but opens no device in a test, and `speech-vad-silero` is not a dependency, so
  neither needs the exclusion `recall-fastembed` needs in almanac.)
