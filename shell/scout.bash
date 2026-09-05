# scout shell integration (canonical copy; ships with the product).
#
# Why this exists: a child process cannot cd its parent shell, so scout
# actions PRINT commands and this function evals them in your shell. The
# eval is guarded: only allowlisted line shapes run (cd / printf /
# $EDITOR-$VISUAL invocations); anything else is shown, never executed.
#
# The printed command goes through a temporary file (`--print-to`) rather
# than a captured stdout, so that in session mode a test runner or REPL
# started by an action keeps stdout for itself. A one-shot run and a
# session both write at most one command; a session that ends with Esc
# writes nothing and nothing is evaluated.
#
# Install: source this file from your shell rc, e.g.
#   source /path/to/scout/shell/scout.bash
#
# Bare `scout`, `scout --session` and `scout -s` run the picker;
# subcommands (index, query, doctor, recon, open-db) pass through untouched.
scout() {
  case "${1:-}" in
    ''|--session|-s)
      local f out line rc
      f="$(mktemp "${TMPDIR:-/tmp}/scout.XXXXXX")" || return 1
      command scout "$@" --print-to "$f"
      rc=$?
      out="$(cat "$f" 2>/dev/null)"
      rm -f "$f"
      [ "$rc" -ne 0 ] && return "$rc"
      [ -z "$out" ] && return 0
      while IFS= read -r line; do
        case "$line" in
          'cd '*|'printf '*|'${EDITOR'*|'${VISUAL'*) ;;
          *) printf 'scout: refusing to eval unexpected output: %s\n' "$line" >&2; return 1 ;;
        esac
      done <<< "$out"
      eval "$out"
      ;;
    *)
      command scout "$@"
      ;;
  esac
}
