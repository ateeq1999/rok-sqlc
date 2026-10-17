# 09: Small correctness fixes

**What to build:**
- **Joined-read aliases:** a column name near PostgreSQL's 63-byte identifier limit can be truncated into a duplicate once it gets the `t_<col>` or `r_<col>` prefix. Use shortened, unique aliases when needed.
- **Default constraint names:** resolve name clashes across the whole schema, the way PostgreSQL does, not per table.
- **`--resource` text:** make the help text and README describe what the flag actually does. It regenerates the named table and keeps every already-generated resource current. It does not touch unrelated files.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] A table with 62-character column names gets a working joined read against PostgreSQL.
- [ ] `ON CONFLICT ON CONSTRAINT` resolves default constraint names across two tables the way PostgreSQL does (CLI seam).
- [ ] The README drift test still passes with the corrected flag text.
