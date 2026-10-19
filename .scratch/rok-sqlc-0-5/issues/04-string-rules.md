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

**Status:** resolved

- [x] Config tests for each rule's parsing and each failure.
- [x] Generator tests for each rule's attribute or function, plus OpenAPI keywords.
- [x] `tests/api-app`: each transform is stored transformed, and each validation rule rejects with its code, against PostgreSQL.

## Comments

Resolved on branch `rok-sqlc-0-5-04`.

- **Config:** `ApiConfig::rules` (`BTreeMap<table, BTreeMap<column, Vec<ApiRule>>>`), `ApiRule` public (`Text`, `Regex`, `In`, `NotIn`, `StartsWith`, `EndsWith`; `From<&str>`). `ApiKeys` reads strings and one-key inline tables (custom deserializer with a clear message) and layers by table, then by column. `src/api_rules.rs` parses and checks: unknown rule (lists all), bad arguments, duplicates, left-out rules (`confirmed`, `sameAs`, `notSameAs`, `activeUrl`), non-text or typed-id columns, managed or generated columns, unknown table or column, invalid `regex` (parsed with the new tool dependency `regex-syntax`).
- **Native `validator` rules** (code overridden to the VineJS name): `email`, `url`, `ipAddress` (`ip`, `v4`/`v6`). **Generated `custom` functions** in `dto.rs` for all others; helpers (`check`, `letters`, `is_uuid`, `is_iban` with validator.js's 80 country formats, `is_credit_card` with Luhn, `is_coordinates`, `normalize_email`, ...) are emitted once per feature, only when used (`src/generator/api/rule_functions.rs`, `rule_helpers.rs`). `minLength`/`maxLength`/`fixedLength` are generated too: a field takes one `validator` `length`, which the schema's declared length already uses.
- **Transforms:** `deserialize_with = "<col>_transformed"` through a generated `Transformed` trait (String, `Option`, and nullable `Option<Option>` keeping `null` = `Some(None)`); `default` added on optional create fields.
- **Regex:** `regex`, `mobile`, `postalCode` use the `regex` crate (already a dependency of `validator`), compiled once in a `LazyLock`; the data layer's `mod.rs` hints `regex = "1"` only then.
- **OpenAPI:** `format` (`email`, `uri`, `ipv4`, `ipv6`, `uuid`), `pattern`, `min_length`/`max_length`; every rule and transform is also listed in the field's doc (description).
- **Deviations:** `mobile` needs a locale (16 carried; VineJS's default `any` isn't) and `postalCode` a country (CA, DE, ES, FR, GB, SA, US; IN's pattern needs look-ahead); `in` can't be an OpenAPI `enum` (utoipa 5 has no field `enum` attribute), so it is in the description; `url` follows `validator` (a scheme is required, unlike validator.js's default); `normalizeEmail` keeps a value with no `@` or an emptied local part as given (validator.js returns `@...`/`false`); lengths count chars, not UTF-16 units. All documented in docs/guide/configuring.md.
- **Fixture:** `tests/api-app` gains `0005_profiles.sql` (public, a rule per column) and `tests/rules.rs` (each transform stored transformed, including on `PATCH` and `null`; each rule's 422 code). `regex = "1"` added to its Cargo.toml (one-line lock change). Expected OpenAPI document regenerated.
- **Template:** `users.email = ["trim", "toLowerCase", "email"]`, `display_name = ["trim"]`. `email` is in no generated body (create skipped, immutable), so only `display_name`'s transform appears in `users/dto.rs`. The contract compares keywords strictly (a served extra keyword is a difference), but the template's rules add none to covered operations, so it is unchanged.
- **Docs:** README reference and prose, configuring guide `rules` section, CHANGELOG, GLOSSARY ("String rule").
