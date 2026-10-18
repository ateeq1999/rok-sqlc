# 03: API configuration

**What to build:** The configuration ADR 0016 describes, validated but not yet used by the generator. A `Config::api: Option<ApiConfig>` holds these settings:

| Setting | Default | Purpose |
|---|---|---|
| `output` | — | the `api_output` directory; when unset, everything below is ignored |
| `db_path` | `crate::db` | path to the data layer |
| `prefix` | `/api/v1` | URL prefix |
| `search_endpoint` | `false` | turns on `POST /search` |
| `exclude` | — | tables |
| `skip` | — | per table, from the endpoint names `list`, `get_one`, `create`, `update`, `remove`, `search`, `restore`, `lookups`, `relationships` |
| `routes` | — | route base per table |
| `access` | `user` | `public`, `user` or `admin`, per table |
| `owner` | — | owner column per table |
| `hidden` | — | columns per table |
| `search` | — | `q` columns per table |
| `sort` | — | sort columns per table |

In the config file this is `[api]` with sub-tables (`[api.skip]`, `[api.routes]`, …). The JSON payload uses the same shape. One flag, `-a/--api-output <DIR>`, sets the output. Layering follows the existing rules: lists replace, table-keyed maps merge by entry.

Checks run after the schema is parsed. Each fails the run with a message naming the key, the table, and the column:
- `api.output` needs `derive = ["serde", "validate"]` and `actix_web` in `extractors`.
- Every table named must be generated.
- Every column named must exist.
- An `owner` column must be `UUID` and `NOT NULL`.
- `search` columns must be text.
- `sort` columns must be sortable (ordered or text filter kinds).
- `hidden` can't name a key column.
- A route base must start with `/` and be unique.
- Unknown endpoint names in `skip` list the valid ones.

`ApiConfig` and its enums are `#[non_exhaustive]` and exported. Document every key and the flag in the README (`config-reference`), so `tests/readme.rs` passes.

**Blocked by:** None

**Status:** ready-for-agent

- [ ] Config-file, JSON, flag, layering and every check covered by CLI or library tests.
- [ ] Output without `[api]` is unchanged, and the README drift tests pass.
