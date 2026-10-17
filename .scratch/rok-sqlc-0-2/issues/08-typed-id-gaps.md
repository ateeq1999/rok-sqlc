# 08: Typed id gaps

**What to build:**
- **Enum primary keys:** these get a typed id wrapping `types::X`. Foreign keys to them use that id.
- **Name collisions:** generation fails with a clear diagnostic in either of these cases:
  - a generated id or key name (`{Struct}Id`, `{Struct}Key`) collides with another generated item in the same module;
  - two imported ids share a name in one file.
- **Same-named tables in different schemas:** these currently collide on module name. Add an opt-in config rename for the module, so both can be generated with typed ids resolving per schema. For example:

  ```toml
  [modules]
  "audit.users" = "audit_users"
  ```

**Blocked by:** None

**Status:** ready-for-agent

- [ ] An enum-keyed table gets an id type, and a foreign key into it uses it (PostgreSQL seam).
- [ ] With a module rename, `core.users` and `audit.users` both generate (CLI seam).
- [ ] Each collision case fails with a diagnostic.
