# scout shell integration (canonical copy — ships with the product).
#
# Why this exists: a child process cannot cd its parent shell, so scout
# actions PRINT commands on stdout and this function evals them in your
# shell. The eval is guarded: only allowlisted line shapes run (cd /
# printf / $EDITOR-$VISUAL invocations); anything else — a bare path, a
# value-printing action, corrupted output — is shown, never executed.
#
# Install: source this file from your shell rc, e.g.
#   source /path/to/scout/shell/scout.bash
#
# Bare `scout` runs the picker; subcommands (index, query, open-db)
# pass through to the binary untouched.
scout() {
  if [ $# -eq 0 ]; then
    local out line
    out="$(command scout)" || return $?
    [ -z "$out" ] && return 0
    while IFS= read -r line; do
      case "$line" in
        'cd '*|'printf '*|'${EDITOR'*|'${VISUAL'*) ;;
        *) printf 'scout: refusing to eval unexpected output: %s\n' "$line" >&2; return 1 ;;
      esac
    done <<< "$out"
    eval "$out"
  else
    command scout "$@"
  fi
}
