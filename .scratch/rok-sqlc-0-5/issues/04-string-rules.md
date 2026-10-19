# 04: String rules and transforms (`[api.rules]`)

**What to build:** the spec's string-rules decision.

**Config:**
- `[api.rules.<table>] <column> = [<rule>, ...]` in the config file and the JSON payload, on `ApiConfig`, layered by table and column;
- a rule is a string (`"email"`, `"minLength(3)"`, `"alphaNumeric(allowDashes, allowSpaces)"`, `"in(SA, AE, EG)"`, `"mobile(ar-SA)"`, `"uuid(4, 7)"`) or an inline table (`{ regex = "^[a-z0-9-]+$" }`, and `{ in = ["a,b", "c"] }` for values with commas);
- the full rule list, transforms, and exclusions are in the spec;
- the config check fails clearly on an unknown rule (listing the valid ones), bad arguments, a rule on a non-text column, a hidden-only conflict if any, and an unknown table or column;
- documented in the README `config-reference` (drift tests).

**Generation:**
- `CreateXDto` and `UpdateXDto` get each rule as a `validator` attribute where `validator` 0.21 has the same meaning (verify each in the registry: `email`, `url`, `length`, `regex`, `contains`…). Otherwise use a generated custom function with code equal to the VineJS name (`mobile`, `iban`, `hexCode`, …).
- Messages are English and name the column.
- **Transforms** (`trim`, `toLowerCase`, `toUpperCase`, `normalizeEmail`) apply during deserialization (`deserialize_with`), so validation sees, and the database stores, the transformed value. `normalizeEmail` follows validator.js's default behaviour for the parts that are well defined (lowercasing; Gmail dot and plus handling only if VineJS's default does it); document exactly what it does.
- **OpenAPI:** where utoipa can express a rule (`format: email`, `format: uri`, `pattern`, `minLength`/`maxLength`, `enum` for `in`), the schema shows it. Other rules appear in the field description.
- **Dependencies:** any new crate (for example a regex engine for `regex`, if `validator`'s needs it, or phone or IBAN validation) goes into the dependency hints, and must be well maintained. Prefer generated code over new dependencies (ADR 0013).

**Template:** give `users` sensible rules (`email`: `email`, `trim`, `toLowerCase`; `display_name`: `trim`). The contract may gain schema keywords the target lacks (`format: email`). Check that the subset contract allows extra keywords, or adjust it to treat added constraints as allowed and record it.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Config tests for each rule's parsing and each failure.
- [ ] Generator tests for each rule's attribute or function, plus OpenAPI keywords.
- [ ] `tests/api-app`: each transform is stored transformed, and each validation rule rejects with its code, against PostgreSQL.
