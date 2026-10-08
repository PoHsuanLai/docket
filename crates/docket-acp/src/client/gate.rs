//! The gate in front of an external agent's calls to us. Every `fs/write_text_file`, every
//! permission request and (through `Terminals`) every command is ruled here before anything
//! happens, and the agent's own word never decides: its "allow" options are not consulted, its
//! "always" is not a grant.
//!
//! The order a call meets: the breaker; a one-use approval from a permission request the person
//! already said yes to; a standing grant of this agent program that covers the call (it replaces
//! only the question); then the person is asked, and "always" is offered only where
//! `may_offer` says so. "Always" stores a standing grant for `GrantCaller::AcpAgent(program)` in
//! our own store; the agent is told `allow_once`.

use super::ask::{AgentAsk, Ask, Shown, What};
use super::breaker::Breaker;
use super::confine::{Care, Confined};
use super::names;
use crate::terminal_ask::{Answer, Posture};
use agent_client_protocol_schema::v1::ToolKind;
use docket_core::{
    AbsPath, AgentReach, AlwaysOffer, ArgFacts, ArgOrigin, AskFacts, AskReason, CallFacts,
    ExecuteFacts, ExecuteRuling, GrantCaller, SandboxState, StandingGrant, StandingGrantId,
    UndoSupport, find_standing, held_with, may_offer, rule_execute,
};
use docket_session::ProgramName;
use prov::{Effect, UnixSeconds};

/// What the ruling rested on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Basis {
    /// A read inside the session's directory: nothing to ask.
    Unasked,
    /// The person already said yes to the permission request for this very thing.
    Approved,
    /// A standing grant stood in for the question.
    Grant(StandingGrantId),
    /// The person said yes now.
    Confirmed,
}

/// Why a call was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    /// The breaker is tripped.
    Breaker,
    /// The person said no, or the question could not be put.
    Declined,
    /// A reviewer's no.
    Reviewer,
    /// Outside what an agent may reach.
    Forbidden,
}

/// What the gate rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ruling {
    /// Go ahead.
    Allow(Basis),
    /// Do not.
    Refuse(Why),
}

/// What the audit keeps of one ruling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Audit {
    /// A call went ahead.
    Allowed {
        /// The action.
        action: String,
        /// On what.
        basis: Basis,
    },
    /// A call was refused.
    Refused {
        /// The action.
        action: String,
        /// Why.
        why: Why,
    },
    /// The person's "always" became a standing grant.
    GrantStored(StandingGrantId),
    /// The agent reported a tool call it ran itself.
    Reported {
        /// The action, `acp.<program>.reported.<kind>`.
        action: String,
    },
}

/// A permission request, read: what the agent says it will do, reduced to what a scope can be
/// compared with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolReq {
    /// The kind it named.
    pub kind: ToolKind,
    /// The call, for matching a grant.
    pub facts: CallFacts,
    /// The effect it is gated as.
    pub effect: Effect,
    /// The agent's own title, as data.
    pub title: Shown,
    /// The command line, for an execute.
    pub line: Option<String>,
    /// The paths it names that lie inside the session's directory (all of them, or the request
    /// is `forbidden`).
    pub paths: Vec<AbsPath>,
    /// Whether it names a path outside the directory or a secret.
    pub forbidden: bool,
}

/// An earlier yes, good for one matching call.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Approval {
    Write(AbsPath),
    Command(String),
}

/// The gate for one agent connection.
#[derive(Debug)]
pub struct Gatekeeper<A> {
    caller: GrantCaller,
    program: ProgramName,
    scope: AbsPath,
    sandbox: SandboxState,
    grants: Vec<StandingGrant>,
    tainted: ArgOrigin,
    breaker: Breaker,
    approvals: Vec<Approval>,
    audit: Vec<Audit>,
    ask: A,
}

impl<A: Ask> Gatekeeper<A> {
    /// A gate for `program` working in `scope`; `sandbox` says whether a command can be
    /// confined (it decides whether an execute may be granted).
    pub fn new(program: ProgramName, scope: AbsPath, sandbox: SandboxState, ask: A) -> Self {
        Self {
            caller: GrantCaller::AcpAgent(program.clone()),
            program,
            scope,
            sandbox,
            grants: Vec::new(),
            tainted: ArgOrigin::Typed,
            breaker: Breaker::default(),
            approvals: Vec::new(),
            audit: Vec::new(),
            ask,
        }
    }

    /// The caller grants are keyed to.
    pub fn caller(&self) -> &GrantCaller {
        &self.caller
    }

    /// The standing grants held.
    pub fn grants(&self) -> &[StandingGrant] {
        &self.grants
    }

    /// Replaces the grants held (from the store, or from the terminal methods after they asked).
    pub fn set_grants(&mut self, grants: Vec<StandingGrant>) {
        self.grants = grants;
    }

    /// The audit lines since the last call.
    pub fn take_audit(&mut self) -> Vec<Audit> {
        std::mem::take(&mut self.audit)
    }

    /// Notes a line for the audit.
    pub fn note(&mut self, line: Audit) {
        self.audit.push(line);
    }

    /// The session read untrusted content (a file, a page): later arguments may derive from it.
    pub fn taint(&mut self) {
        self.tainted = ArgOrigin::Untrusted;
    }

    /// Whether the session is tainted.
    pub fn tainted(&self) -> bool {
        self.tainted == ArgOrigin::Untrusted
    }

    /// The breaker.
    pub fn breaker(&mut self) -> &mut Breaker {
        &mut self.breaker
    }

    /// What the terminal methods are told about the session before each command.
    pub fn posture(&self) -> Posture {
        Posture {
            origin: self.tainted,
            breaker: self.breaker.state(),
            ..Posture::default()
        }
    }

    /// The person spoke: the breaker and every unused approval start again. Taint stays.
    pub fn new_turn(&mut self) {
        self.breaker.new_turn();
        self.approvals.clear();
    }

    /// Whether an approval for this write is held, and if so it is spent.
    pub fn spend_write(&mut self, path: &AbsPath) -> bool {
        self.spend(&Approval::Write(path.clone()))
    }

    /// Whether an approval for this command line is held, and if so it is spent.
    pub fn spend_command(&mut self, line: &str) -> bool {
        self.spend(&Approval::Command(line.to_owned()))
    }

    fn spend(&mut self, wanted: &Approval) -> bool {
        match self.approvals.iter().position(|a| a == wanted) {
            Some(at) => {
                self.approvals.remove(at);
                true
            }
            None => false,
        }
    }

    fn finish(&mut self, action: &str, ruling: Ruling) -> Ruling {
        let action = action.to_owned();
        match &ruling {
            Ruling::Allow(basis) => {
                // Only a question that reached the person counts toward a flood; a read, an
                // approval spent and a grant used cost them nothing.
                if *basis == Basis::Confirmed {
                    self.breaker.request();
                }
                self.breaker.allowed();
                self.audit.push(Audit::Allowed {
                    action,
                    basis: basis.clone(),
                });
            }
            Ruling::Refuse(why) => {
                self.breaker.request();
                self.breaker.denied();
                self.audit.push(Audit::Refused { action, why: *why });
            }
        }
        ruling
    }

    async fn put(&mut self, now: UnixSeconds, question: AgentAsk) -> Ruling {
        let offer = question.offer.clone();
        match self.ask.ask(&question).await {
            Answer::No => Ruling::Refuse(Why::Declined),
            Answer::Once => Ruling::Allow(Basis::Confirmed),
            Answer::Always => {
                if let AlwaysOffer::Offered(scope) = offer {
                    let grant = StandingGrant::new(self.caller.clone(), scope, now);
                    self.audit.push(Audit::GrantStored(grant.id.clone()));
                    self.grants = held_with(std::mem::take(&mut self.grants), grant);
                }
                Ruling::Allow(Basis::Confirmed)
            }
        }
    }

    /// Rules a write the agent asked us to make (`fs/write_text_file`).
    pub async fn write(&mut self, now: UnixSeconds, id: u64, file: &Confined) -> Ruling {
        let action = names::action(&self.program, "edit");
        let name = action
            .as_ref()
            .map_or_else(String::new, |a| a.name.as_str().to_owned());
        if self.breaker.state() == docket_core::BreakerState::Tripped {
            return self.finish(&name, Ruling::Refuse(Why::Breaker));
        }
        let Some(action) = action else {
            return self.finish(&name, Ruling::Refuse(Why::Forbidden));
        };
        if self.spend_write(&file.path) {
            return self.finish(&name, Ruling::Allow(Basis::Approved));
        }
        let facts = CallFacts {
            action,
            args: ArgFacts::Paths(vec![file.path.clone()]),
        };
        if file.care == Care::Plain
            && let Some(grant) = find_standing(&self.grants, &self.caller, &facts)
        {
            let basis = Basis::Grant(grant.id.clone());
            return self.finish(&name, Ruling::Allow(basis));
        }
        let offer = self.offer(&facts, Effect::UndoableWrite, file.care);
        let question = AgentAsk {
            id,
            what: What::Write(file.path.clone()),
            title: Shown::of(file.path.as_str()),
            offer,
        };
        let ruling = self.put(now, question).await;
        self.finish(&name, ruling)
    }

    fn offer(&self, facts: &CallFacts, effect: Effect, care: Care) -> AlwaysOffer {
        let why = if self.tainted() {
            vec![AskReason::Effect(effect), AskReason::Tainted]
        } else {
            vec![AskReason::Effect(effect)]
        };
        let ask = AskFacts {
            effect,
            undo: if effect == Effect::UndoableWrite {
                UndoSupport::Token
            } else {
                UndoSupport::NotUndoable
            },
            reach: match care {
                Care::Plain => AgentReach::Offered,
                Care::Sensitive => AgentReach::AskAlways,
            },
            why: &why,
            untrusted: &[],
            breaker: self.breaker.state(),
            budget: docket_core::BudgetState::Within,
        };
        may_offer(&self.caller, facts, &ask)
    }

    /// Rules a permission request. Allowing it makes an approval that the matching `fs` write or
    /// command then spends.
    pub async fn tool(&mut self, now: UnixSeconds, id: u64, tool: &ToolReq) -> Ruling {
        let name = tool.facts.action.name.as_str().to_owned();
        if self.breaker.state() == docket_core::BreakerState::Tripped {
            return self.finish(&name, Ruling::Refuse(Why::Breaker));
        }
        if tool.forbidden {
            return self.finish(&name, Ruling::Refuse(Why::Forbidden));
        }
        let ruling = match tool.kind {
            ToolKind::Read | ToolKind::Search | ToolKind::Think => Ruling::Allow(Basis::Unasked),
            ToolKind::Execute => self.execute(now, id, tool).await,
            _ => self.general(now, id, tool).await,
        };
        if matches!(ruling, Ruling::Allow(_)) {
            self.approve(tool);
        }
        self.finish(&name, ruling)
    }

    fn approve(&mut self, tool: &ToolReq) {
        match (tool.kind, &tool.line) {
            (ToolKind::Execute, Some(line)) => self.approvals.push(Approval::Command(line.clone())),
            (ToolKind::Edit | ToolKind::Move, _) => {
                let paths = tool.paths.iter().cloned().map(Approval::Write);
                self.approvals.extend(paths);
            }
            _ => {}
        }
    }

    async fn general(&mut self, now: UnixSeconds, id: u64, tool: &ToolReq) -> Ruling {
        if let Some(grant) = find_standing(&self.grants, &self.caller, &tool.facts) {
            return Ruling::Allow(Basis::Grant(grant.id.clone()));
        }
        let offer = self.offer(&tool.facts, tool.effect, Care::Plain);
        let question = AgentAsk {
            id,
            what: What::Tool {
                kind: names::tail(tool.kind),
                paths: tool.paths.clone(),
            },
            title: tool.title.clone(),
            offer,
        };
        self.put(now, question).await
    }

    async fn execute(&mut self, now: UnixSeconds, id: u64, tool: &ToolReq) -> Ruling {
        let facts = ExecuteFacts {
            caller: &self.caller,
            call: &tool.facts,
            sandbox: self.sandbox,
            origin: self.tainted,
            breaker: self.breaker.state(),
            budget: docket_core::BudgetState::Within,
            review: None,
        };
        match rule_execute(&facts, &self.grants) {
            ExecuteRuling::Deny => Ruling::Refuse(Why::Reviewer),
            ExecuteRuling::Run(grant) => Ruling::Allow(Basis::Grant(grant)),
            ExecuteRuling::Ask { offer, .. } => {
                let line = tool.line.clone().unwrap_or_default();
                let question = AgentAsk {
                    id,
                    what: What::Command {
                        line,
                        cwd: self.scope.clone(),
                    },
                    title: tool.title.clone(),
                    offer,
                };
                self.put(now, question).await
            }
        }
    }
}
