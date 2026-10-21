// The permissions grid, one for both modes. A conversation is in Auto or Manual, and Settings
// holds five rows for each mode (`agent.auto.*`, `agent.manual.*`). The router resolves the rows
// of the call's mode and hands them to Cedar as `context.rows`; this grid permits on them, so
// there is one set of rules and the rows pick the rules that apply. `context.mode` is there for
// a person's own rule that should hold in one mode only.
//
// Cedar is default-deny and forbid overrides permit. Three queries run per call (see
// quire.cedarschema): `perform` not permitted is Deny, `perform_unasked` not permitted is Ask,
// `perform_unjudged` not permitted is AllowJudged, all permitted is AllowFinal. A row that
// "asks" is the absence of a permit; "check" permits `perform_unasked` only, so a reviewer
// judges the call; "runs" permits both.
//
// Layout: the permit rules say how far each cell gets (every permit names the strictest query it
// satisfies); the forbid rules below ask. The invariants that refuse or hold in every mode
// (Rule of Two, untrusted sinks, hidden actions, denied consent, lasting memory, limits) are
// `core.cedar`'s; no row, task policy or reviewer can lift them. "T" below means the planner and
// every sink argument are trusted; "In" means the call is inside the task policy.
//
//   Effect / condition                       Row                        Result
//   Read                                     (none)                     final, with no question
//   Undoable write, T, In                    inside = runs              final
//   Undoable write, anything else            other = check              judged (a reviewer may tighten)
//   Undoable write, a step of a screen run
//     the person allowed                     screen = once              final
//   Outbound, T recipient and planner        send = runs                final
//   Destructive, T path and planner          delete = runs              final
//   Any change in a screen run               screen = each              ask
//   Any other cell                           (a row that asks)          ask
//
// The assistant's own bookkeeping (the Companion and Memory apps) never asks for a first use.
// First use asks only for a change, never for a read. The terminal (`quire-do`, principal kind
// "cli") has its own rows: a read is final; an undoable, outbound or destructive act asks and
// never goes to a reviewer ("no auto-approval for a terminal"), unless the person gave a standing
// grant for that action from the terminal, in which case it is final; a destructive act always
// asks; a hidden action is denied (hidden-from-agents).
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

// An undoable write with trusted arguments inside the task policy is final.
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
  context.rows.inside == "runs" &&
  context.coverage == "inside" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted" &&
  context.sinks.destination == "trusted" &&
  context.sinks.body == "trusted" &&
  context.sinks.path == "trusted"
};

// Any other undoable write is judged when "other" says check. What the inside row would have
// let through, that row asks about instead: it is not "other".
@id("undoable-judged")
permit (
  principal,
  action == Quire::Action::"perform_unasked",
  resource
)
when {
  resource.effect == "undoable_write" &&
  context.rows.other == "check"
}
unless {
  context.coverage == "inside" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted" &&
  context.sinks.destination == "trusted" &&
  context.sinks.body == "trusted" &&
  context.sinks.path == "trusted"
};

// Sending, when the person said it goes ahead: the planner and the recipient are trusted. An
// untrusted sink, the Rule of Two and the first-use question still ask (core and the forbids
// below).
@id("outbound-final")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when {
  resource.effect == "outbound" &&
  context.rows.send == "runs" &&
  context.planner == "trusted" &&
  context.sinks.recipient == "trusted"
};

// Deleting, when the person said it goes ahead: the planner and the path are trusted.
@id("destructive-final")
permit (
  principal,
  action in [
    Quire::Action::"perform_unasked",
    Quire::Action::"perform_unjudged"
  ],
  resource
)
when {
  resource.effect == "destructive" &&
  context.rows.delete == "runs" &&
  context.planner == "trusted" &&
  context.sinks.path == "trusted"
};

// A step of a screen run the person allowed (the start sheet, or a standing grant for the app)
// that only does something undoable runs, when the screen row says it asks once. A step that sends
// or deletes gets no permit here and asks.
@id("screen-step-final")
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
  context.run == "allowed" &&
  context.rows.screen == "once"
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
@why("This reaches into a different Space than the one you are working in.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { context.target_space == "other" && principal.kind != "user" };

// More things than the mass threshold (a read touches nothing).
@id("mass")
@why("This changes a lot of things at once.")
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
@why("This app asks every time before it does this.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { resource.reach == "ask_always" && principal.kind != "user" };

// A terminal cannot tell the person from an agent typing in it: anything but a read asks unless
// the person gave a standing grant for this action from the terminal.
@id("cli-asks")
@why("A terminal cannot tell you from a program typing in it, so it asks.")
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
@why("Deleting from a terminal always asks.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { principal.kind == "cli" && resource.effect == "destructive" };

// A screen run on a row that asks at each change: every change of the run asks, whatever else
// would let it through.
@id("screen-each-asks")
@why("You chose to be asked at each change in another app's window.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  context.run == "allowed" &&
  context.rows.screen == "each" &&
  resource.effect != "read" &&
  principal.kind != "user"
};

// First use of this app, class and Space, or a grant that asks once, asks for a change, never for a
// read. It does not apply to the assistant's own bookkeeping (the Companion and Memory apps: a
// task start or message, a recall, a staged memory; anything that sends or deletes is not
// covered), nor to a step of a screen run the person already allowed when the screen row asks once. The terminal has no class
// grants: a read from it is allowed and everything else asks by the rules above.
@id("first-use")
@why("This is the first time it has been allowed to do this here.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when {
  (context.grant == "none" || context.grant == "once") &&
  resource.effect != "read" &&
  principal.kind != "user" &&
  principal.kind != "cli"
}
unless {
  (context.run == "allowed" && context.rows.screen == "once") ||
  ((resource in Quire::App::"org.quire.Companion" ||
    resource in Quire::App::"org.quire.Memory") &&
   (resource.effect == "undoable_write"))
};

// An external MCP client may read; anything else asks. An external coding agent's host is not an
// MCP client: its calls (the `org.quire.AcpAgent` pseudo-app, which only that role may call) are
// ruled by the grid above and by taint, so a standing grant of the program can stand in for the
// ask where the named rules allow.
@id("mcp-asks")
@why("Another app asked for this, so it checks with you before changing anything.")
forbid (
  principal,
  action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"],
  resource
)
when { principal.kind == "mcp" && resource.effect != "read" }
unless { resource in Quire::App::"org.quire.AcpAgent" };
