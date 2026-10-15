# 01: `--help`/`--version` exit 0, and every command and flag gets a short alias

**What to build:** `rok-sqlc --help`, `rok-sqlc --version`, and `rok-sqlc generate --help` print their output and exit successfully. Today they go through the error path and exit 1. Every command and flag has a short alias: `generate`→`g`, `--schema`→`-s`, `--output`→`-o`, `--resource`→`-r`, `--queries`→`-q`, `--dry-run`→`-n`, `--force`→`-f`, `--lenient`→`-l`, `--help`→`-h`, `--version`→`-V`. Flags introduced by later tickets (`-j`, `-c`, `-k`) take their reserved letters when they land.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Running the binary with `--help`, `-h`, `--version`, `-V`, and `g --help` exits 0 and prints to stdout (CLI seam, binary invocation).
- [x] `rok-sqlc g -s schema.sql -o out` behaves exactly like the long form.
- [x] A CLI test walks the command definition and fails if any command or flag lacks a short alias, or if two aliases collide.
- [x] Existing CLI tests still pass.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (90dc138, 0f581f5). Help/version exit 0 via stdout; `g` and all short flags added; alias-walk tests in `tests/cli.rs` reserve `-j`/`-c`/`-k`. Code review follow-up: clap usage errors now print once (no `Error: error:` prefix) and still exit 1, since the spec defines no usage-error code. `cli::command()` is `#[doc(hidden)]`, so ticket 02 can decide whether it stays public.
