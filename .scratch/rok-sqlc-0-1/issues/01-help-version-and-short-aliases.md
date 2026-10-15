# 01: `--help`/`--version` exit 0, and every command and flag gets a short alias

**What to build:** `rok-sqlc --help`, `rok-sqlc --version`, and `rok-sqlc generate --help` print their output and exit successfully. Today they go through the error path and exit 1. Every command and flag has a short alias: `generate`→`g`, `--schema`→`-s`, `--output`→`-o`, `--resource`→`-r`, `--queries`→`-q`, `--dry-run`→`-n`, `--force`→`-f`, `--lenient`→`-l`, `--help`→`-h`, `--version`→`-V`. Flags introduced by later tickets (`-j`, `-c`, `-k`) take their reserved letters when they land.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Running the binary with `--help`, `-h`, `--version`, `-V`, and `g --help` exits 0 and prints to stdout (CLI seam, binary invocation).
- [ ] `rok-sqlc g -s schema.sql -o out` behaves exactly like the long form.
- [ ] A CLI test walks the command definition and fails if any command or flag lacks a short alias, or if two aliases collide.
- [ ] Existing CLI tests still pass.
