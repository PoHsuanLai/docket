// The default policy set. The two named hard rules below are written and validated at the
// freeze; the grid of addendum A2 (permit rules per effect, strictness and condition) arrives
// with the policy point's fill, where `default_policy_grid` pins one row per cell. Cedar is
// default-deny and forbid overrides permit, so these two hold in every strictness once any
// permit exists: no task policy and no reviewer can lift them.

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
