// The Manual preset: for when the person is watching in real time. Cedar is default-deny and
// forbid overrides permit. Three queries run per call (see quire.cedarschema): `perform` not
// permitted is Deny, `perform_unasked` not permitted is Ask, `perform_unjudged` not permitted is
// AllowJudged, all permitted is AllowFinal.
//
// Layout: the permit rules say what runs without a question; the forbid rules below ask. The
// invariants that refuse or hold in every preset are `core.cedar`'s.
//
//   Effect / condition                          Manual
//   Read                                        final, with no question
//   Undoable write, outbound, destructive       ask, even inside the task and even for a step of
//                                               a screen run
//
// Nothing is reviewed silently: there is no permit that leaves a call to a reviewer, so no call
// is AllowJudged. A standing grant the person gave from the terminal (`quire-do`, principal kind
// "cli") still makes that one action final, as in Auto; a destructive act from a terminal always
// asks; a hidden action is denied (hidden-from-agents).

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

// First use of this app, class and Space, or a grant that asks once, asks for a change, never for a
// read. The terminal has no class grants: a read from it is allowed and everything else asks by
// the rules above.
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
