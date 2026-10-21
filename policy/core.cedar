// The core policy set: the safety invariants that hold in every preset and that no later layer
// (preset, person, assistant, administrator) may lift. It is compiled in. Cedar is default-deny
// and forbid overrides permit, so a rule here cannot be undone by a permit written anywhere else.
// Three queries run per call (see quire.cedarschema); the grid that says how far each cell gets
// is the preset's (`presets/standard/grid.cedar`).

// ---- Forbids that refuse (Deny) ----------------------------------------------------------

// An action the app keeps for the person is not for an agent.
@id("hidden-from-agents")
forbid (principal, action, resource)
when { resource.reach == "hidden" && principal.kind != "user" };

// A denied consent stays denied.
@id("grant-denied")
forbid (principal, action, resource)
when { context.grant == "denied" && principal.kind != "user" };

// An external MCP client never reaches into another Space.
@id("mcp-other-space")
forbid (principal, action, resource)
when { principal.kind == "mcp" && context.target_space == "other" };

// Putting a workspace back is the person's. An MCP client and an external coding agent never
// restore, whatever the strictness (the router's role table refuses them first; this holds if a
// role were ever added to it).
@id("checkpoints-not-for-agents")
forbid (principal, action, resource)
when {
  (principal.kind == "mcp" || principal.kind == "acp") &&
  resource in Quire::App::"org.quire.Checkpoints"
};

// The facts below are gathered by the router (the kill switch, the session's ledger, the
// breaker, the limits) and ruled on here, so a refusal is always a Cedar ruling. The router only
// words it: `halted` as "stopped", `over-budget` as the budget that ran out, `repeated` as "the
// same call was already refused", `limit-never` as "you said never".

// The Space is stopped by the kill switch.
@id("halted")
forbid (principal, action == Quire::Action::"perform", resource)
when { context.halt == "halted" };

// A budget of the session would be over.
@id("over-budget")
forbid (principal, action == Quire::Action::"perform", resource)
when { context.budget != "ok" };

// The exact call was denied before.
@id("repeated")
forbid (principal, action == Quire::Action::"perform", resource)
when { context.repeat == "repeated" };

// A limit says never (the person's, the assistant's or the administrator's floor): refused
// without asking, whatever is granted.
@id("limit-never")
forbid (principal, action == Quire::Action::"perform", resource)
when { context.limit == "never" };

// ---- Forbids that ask --------------------------------------------------------------------

// Rule of Two: private data, untrusted input and an outbound channel never meet in one
// session without the person.
@id("rule-of-two")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  resource.effect == "outbound" &&
  context.saw_private == "seen" &&
  context.saw_untrusted == "seen"
};

// Arguments are gated by the integrity of the sink they feed, never by string matching: an
// untrusted value in a recipient, destination, path or body of an outbound or destructive act
// asks, even inside a task policy.
@id("untrusted-sink")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  (resource.effect == "outbound" || resource.effect == "destructive") &&
  (context.sinks.recipient == "untrusted" ||
   context.sinks.destination == "untrusted" ||
   context.sinks.path == "untrusted" ||
   context.sinks.body == "untrusted")
};

// Lasting memory from untrusted arguments.
@id("lasting-from-untrusted")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  resource.lasting == "agent_memory" &&
  context.args == "untrusted" &&
  principal.kind != "user"
};

// A limit says always ask: no grant, standing grant or approval stands in for the question.
@id("limit-ask")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { context.limit == "ask" };
