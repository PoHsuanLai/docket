// The standard preset: the grid. Cedar is default-deny and forbid overrides permit. Three queries run per
// call (see quire.cedarschema): `perform` not permitted is Deny, `perform_unasked` not permitted
// is Ask, `perform_unjudged` not permitted is AllowJudged, all permitted is AllowFinal.
//
// Layout: the permit rules say how far each cell of the grid of actions-addendum A2 gets (every
// permit names the strictest query it satisfies); the forbid rules below ask. The invariants that
// refuse or hold in every preset (Rule of Two, untrusted sinks, hidden actions, denied consent,
// lasting memory) are `core.cedar`'s; no task policy and no reviewer can lift them. "T" below means
// the planner and every sink argument are trusted; "In" means the call is inside the task policy.
//
//   Effect / condition           AskMore       Default                      TrustMore
//   Read, same Space             AllowFinal    AllowFinal                   AllowFinal
//   UndoableWrite, T, In         AllowJudged   AllowFinal                   AllowFinal
//   UndoableWrite, otherwise     Ask           AllowJudged                  AllowJudged
//   Outbound, T, In              Ask           Ask (QUESTIONS S1)           AllowJudged + second opinion
//   Outbound, otherwise          Ask           Ask                          Ask
//   Destructive, T, In           Ask           Ask                          AllowJudged + second opinion
//   Destructive, otherwise       Ask           Ask                          Ask
//
// The terminal (`quire-do`, principal kind "cli") has its own rows, the same in every strictness:
// a read is final; an undoable, outbound or destructive act asks and never goes to a reviewer
// ("no auto-approval for a terminal"), unless the person gave a standing grant for that action
// from the terminal, in which case it is final (the named rules below still ask); a destructive
// act always asks; a hidden action is denied (hidden-from-agents).
//
// The second opinion is the cascade's business (`action_review::plan`), not Cedar's.

// ---- Permits: the grid -------------------------------------------------------------------

// Everything is permitted to be attempted; the forbids below and the absence of the stricter
// permits decide what happens to it. Without this, a call with no stricter permit would be a
// Deny instead of an Ask.
@id("attempt")
permit (
  principal,
  action in [
    Quire::Action::"perform",
    Quire::Action::"search",
    Quire::Action::"preview",
    Quire::Action::"suggest",
    Quire::Action::"read_context",
    Quire::Action::"undo",
    Quire::Action::"index_push"
  ],
  resource
);

// A read is final; the Space, consent, reach and mass rules below still ask.
@id("read-final")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when { resource.effect == "read" };

// The person's own act from the launcher skips policy (QUESTIONS S6): the app's own alerts apply.
@id("user-launcher")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when { principal.kind == "user" && context.origin == "launcher" };

// An undoable write with trusted arguments inside the task policy is final in Default and
// TrustMore.
@id("undoable-trusted-inside-final")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when {
  resource.effect == "undoable_write" &&
  context.coverage == "inside" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted" &&
  context.sinks.destination == "trusted" &&
  context.sinks.body == "trusted" &&
  context.sinks.path == "trusted" &&
  (context.strictness == "default" || context.strictness == "trust_more")
};

// An undoable write is judged: always in Default and TrustMore, and in AskMore when it is
// trusted and inside.
@id("undoable-judged")
permit (
  principal,
  action == Quire::Action::"perform_unasked",
  resource
)
when {
  resource.effect == "undoable_write" &&
  (context.strictness != "ask_more" ||
   (context.coverage == "inside" &&
    context.planner == "trusted" &&
    context.sinks.recipient == "trusted" &&
    context.sinks.destination == "trusted" &&
    context.sinks.body == "trusted" &&
    context.sinks.path == "trusted"))
};

// Outbound, trusted and inside the task policy, is judged (with a second opinion) only under
// TrustMore. QUESTIONS S1: under Default and AskMore, Outbound always asks.
@id("outbound-trusted-inside-judged")
permit (
  principal,
  action == Quire::Action::"perform_unasked",
  resource
)
when {
  resource.effect == "outbound" &&
  context.strictness == "trust_more" &&
  context.coverage == "inside" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted" &&
  context.sinks.destination == "trusted" &&
  context.sinks.body == "trusted" &&
  context.sinks.path == "trusted"
};

// Destructive, trusted and inside the task policy, is judged only under TrustMore.
@id("destructive-trusted-inside-judged")
permit (
  principal,
  action == Quire::Action::"perform_unasked",
  resource
)
when {
  resource.effect == "destructive" &&
  context.strictness == "trust_more" &&
  context.coverage == "inside" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted" &&
  context.sinks.destination == "trusted" &&
  context.sinks.body == "trusted" &&
  context.sinks.path == "trusted"
};

// The terminal: the person gave a standing grant for this action ("allow from the terminal: this
// action, until logout"), so it is final. No reviewer sits between a terminal and the app. The
// forbids below (Rule of Two, untrusted sinks, mass, ask-always, lasting memory) still ask.
@id("cli-granted-final")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when {
  principal.kind == "cli" &&
  resource.effect != "read" &&
  context.terminal == "granted"
};

// ---- Forbids that ask --------------------------------------------------------------------

// The target lives in another Space.
@id("cross-space")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { context.target_space == "other" && principal.kind != "user" };

// More things than the mass threshold (a read touches nothing).
@id("mass")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  resource.effect != "read" &&
  context.count > context.mass_at &&
  principal.kind != "user"
};

// An action the manifest says asks every time.
@id("ask-always")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { resource.reach == "ask_always" && principal.kind != "user" };

// A terminal cannot tell the person from an agent typing in it: anything but a read asks unless
// the person gave a standing grant for this action from the terminal.
@id("cli-asks")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  principal.kind == "cli" &&
  resource.effect != "read" &&
  context.terminal == "not_granted"
};

// A standing terminal grant never covers a destructive act.
@id("cli-destructive-asks")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { principal.kind == "cli" && resource.effect == "destructive" };

// First use of this app, class and Space, or a grant that asks once. The terminal has no class
// grants: a read from it is allowed and everything else asks by the rules above.
@id("first-use")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  (context.grant == "none" || context.grant == "once") &&
  principal.kind != "user" &&
  principal.kind != "cli"
};

// A session that read untrusted content cannot lean on an "always" grant for anything that
// changes something: it asks once more. Reading stays free, or one mail read would put a
// question on every read after. (The terminal is held to the same: a recorded always-grant for
// it is no more standing than an agent's.)
@id("tainted-always-asks")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  context.grant == "always" &&
  context.taint == "tainted" &&
  resource.effect != "read" &&
  principal.kind != "user"
};

// An external MCP client may read; anything else asks. An external coding agent's host is not an
// MCP client: its calls (the `org.quire.AcpAgent` pseudo-app, which only that role may call) are
// ruled by the grid above and by taint, so a standing grant of the program can stand in for the
// ask where the named rules allow.
@id("mcp-asks")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { principal.kind == "mcp" && resource.effect != "read" }
unless { resource in Quire::App::"org.quire.AcpAgent" };
