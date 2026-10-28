#!/usr/bin/env bash
# docket-core's modules against the crate stack they are planned to split into (agent-layers
# CORE-CUT-DESIGN.md, section 2). Levels 0 to 3, docket-vocab, docket-manifest,
# docket-permissions-core and companion-model, and levels 5 and 6, docket-gate-wire and docket-wire
# (both above docket-core), are their own crates now (the boundary check keeps them in place), so
# their modules are no longer on disk here; an import of them is a crate edge, not a `crate::` path. Each module has a level; a module may import only modules at
# its own level or below. An import of a higher level is an upward edge and fails the check.
# Non-test code only: everything after the first `#[cfg(test)]` in a file is dropped.
# Items imported by root name (`crate::TaskPolicy`) are traced to their module through lib.rs.
set -uo pipefail
cd "$(dirname "$0")/.."
src=crates/docket-core/src

# level <crate>: modules
LEVELS=(
  "4 docket-core: audit brief budget spend call checkpoint compact config confirm context derived external handover index memory_link message planner preview reader roster run_end schema skill trace undo"
)

map=$(mktemp) || exit 1
trap 'rm -f "$map"' EXIT
for row in "${LEVELS[@]}"; do
  lvl=${row%% *}; rest=${row#* }; crate=${rest%%:*}
  for m in ${rest#*:}; do printf '%s %s %s\n' "$m" "$lvl" "$crate"; done
done >"$map"

# A module on disk without a level is an error: a new module must be placed in the stack.
bad=0
for f in "$src"/*.rs "$src"/*/; do
  m=$(basename "$f" .rs)
  [ "$m" = lib ] && continue
  grep -q "^$m " "$map" || { echo "UNMAPPED: $m has no level in scripts/check-core-layers.sh"; bad=1; }
done

edges=$(perl -w -0777 -e '
  my ($map, $lib, @files) = @ARGV;
  my (%lvl, %crate, %item);
  open my $m, "<", $map or die; { local $/ = "\n"; while (<$m>) { my ($n, $l, $c) = split; $lvl{$n} = $l; $crate{$n} = $c; } }
  open my $l, "<", $lib or die; local $/; my $t = <$l>;
  while ($t =~ /pub use (\w+)::(\{[^}]*\}|\w+)\s*;/g) {
    my ($mod, $body) = ($1, $2);
    $body =~ s/[{}]//g;
    for my $i (split /,/, $body) {
      $i =~ s/^\s+|\s+$//g; next if $i eq "";
      my $name = $i =~ /\bas\s+(\w+)$/ ? $1 : $i;
      $item{$name} = $mod;
    }
  }
  for my $f (@files) {
    open my $h, "<", $f or die; my $s = <$h>;
    $s =~ s/#\[cfg\(test\)\].*//s;
    $s =~ s{//[^\n]*}{}g;
    (my $rel = $f) =~ s{.*/src/}{}; my ($from) = $rel =~ m{^(\w+)};
    my %seen;
    while ($s =~ /\bcrate::(\w+|\{)/g) {
      my @names;
      if ($1 eq "{") {
        my $start = pos($s); my $depth = 1; my $i = $start;
        while ($depth > 0 && $i < length $s) { my $c = substr($s, $i++, 1); $depth++ if $c eq "{"; $depth-- if $c eq "}"; }
        my $body = substr($s, $start, $i - $start - 1); my $d = 0; my $cur = "";
        for my $c (split //, $body) {
          if ($c eq "{") { $d++ } elsif ($c eq "}") { $d-- }
          if ($c eq "," && $d == 0) { push @names, $cur; $cur = "" } elsif ($d == 0 || $c ne " ") { $cur .= $c }
        }
        push @names, $cur;
        @names = map { /(\w+)/ ? $1 : () } @names;
      } else { @names = ($1) }
      for my $n (@names) {
        my $to = exists $lvl{$n} ? $n : $item{$n};
        next unless defined $to && exists $lvl{$to} && $to ne $from;
        next if $seen{$to}++;
        next unless exists $lvl{$from};
        print "$from ($crate{$from}) -> $to ($crate{$to})\n" if $lvl{$to} > $lvl{$from};
      }
    }
  }
' "$map" "$src/lib.rs" $(find "$src" -name '*.rs' ! -name lib.rs | sort)) || { echo "ERROR: edge scan failed"; exit 1; }

if [ -n "$edges" ]; then
  echo "UPWARD EDGES in docket-core:"
  sort -u <<<"$edges" | sed 's/^/    /'
  echo "upward edges: $(sort -u <<<"$edges" | wc -l)"
  exit 1
fi
[ "$bad" -eq 0 ] && echo "core layers hold: 0 upward edges across $(wc -l <"$map") modules in 1 planned crate"
exit "$bad"
