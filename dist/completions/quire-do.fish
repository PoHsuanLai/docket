# fish completion for quire-do. What to offer is quire-do's own answer (`quire-do __complete`),
# so apps, actions, parameter flags and choices follow the installed manifests.
function __quire_do_complete
    set -l typed (commandline -opc)[2..-1] (commandline -ct)
    quire-do __complete $typed 2>/dev/null
end
complete -c quire-do -f -a '(__quire_do_complete)'
