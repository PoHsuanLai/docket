# bash completion for quire-do. What to offer is quire-do's own answer (`quire-do __complete`),
# so apps, actions, parameter flags and choices follow the installed manifests.
_quire_do() {
    local IFS=$'\n'
    COMPREPLY=($(quire-do __complete "${COMP_WORDS[@]:1:$COMP_CWORD}" 2>/dev/null))
}
complete -F _quire_do quire-do
