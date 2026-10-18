# 07: Access, ownership, and hidden columns

**What to build:** The access part of ADR 0016's wire standard, on the features from ticket 06:

- **Access:** each table's handlers take `AuthUser` (`api.access` `user`, the default), `AdminUser` (`admin`), or neither (`public`). Both extractors come from the scaffolded `shared/auth.rs`.
- **Ownership** (`api.owner`): the table's queries are scoped to the caller's id (`WHERE … AND owner = $n`) in new or extended `db` functions. `create` sets the owner from `AuthUser` and takes no owner field in `CreateXDto`, and `UpdateXDto` can't change it. Another caller's row is a 404. The service trait's methods take the `AuthUser` where ownership or access needs it.
- **Hidden columns** (`api.hidden`): left out of `XResponse`. A hidden column stays in `CreateXDto` and `UpdateXDto` (the service may transform it, e.g. hash a password). The `sort` param can't name one (so it gets no `XSort` variant). If ticket 08's search endpoint is on, it's rejected there. Coordinate by leaving a clear hook.

**Blocked by:** 06

**Status:** resolved

- [x] The `tests/api-app` fixture covers each access level (401 without a token, 403 for a non-admin), owner scoping (another user's row gives 404, create sets the owner), and that hidden columns never appear in a response.
- [x] The lint gate passes.

## Comments

Resolved on branch `rok-sqlc-0-4-07`.

**Model.** `Feature` (`src/generator/api/feature.rs`) gained `access: ApiAccess`, `owner: Option<&Column>` and `hidden: Vec<&Column>`, plus `caller()` (`Some("AuthUser" | "AdminUser")`, `None` for public), `is_hidden(column)`, `response_columns()` and `reads_owned_rows()` (owner and an endpoint taking a key). `Update` is served only when a patch column other than the owner exists.

**Access.** Handlers take the caller right after `service`: `_: AuthUser` / `_: AdminUser` when not owned (the extractor alone gives 401/403); `user: AuthUser` / `admin: AdminUser` when owned, passed to the service (`admin.0`). Service methods take `user: AuthUser` (after `conn` for `_in`) only on owned features.

**Ownership.** Data layer (`db/<table>/listing.rs`, now generated for a list *or* owned rows): `list_matching`/`count_matching` take `owner: uuid::Uuid` first (` AND owner = ` bind); `get_owned(key, owner)`; `lock_owned(key, owner) -> bool` (`SELECT 1 … FOR UPDATE`). `update_in`/`remove_in` call `lock_owned(&mut *conn, …)` then the plain `update`/`delete` (404 if not owned). `CreateXDto` drops the owner and gets `into_new(self, owner: uuid::Uuid) -> NewX` instead of `From`; `UpdateXDto` drops it (`From` sets it `None`).

**Hidden.** Left out of `XResponse`; kept in both bodies; excluded from default sort keys.

**Config checks** (`api_config::check`): owner on a `public` table fails; `[api.search]`/`[api.sort]` naming a hidden column fails. README FULL_API test now uses `posts = "user"`, `audit_log = "public"`.

**For 08.** Search/lookups: reject hidden columns via `feature.is_hidden` (no `by-` route, no filter field). New endpoints on owned features must take `user` and scope through owner-aware db functions (follow `get_owned`/`lock_owned`); tenant scoping will need placeholder renumbering in `render_owned` (it assumes no tenant: owner is `$keys+1`). The `mod.rs` doc of `listing` changed to "The API feature's reads: its list page and sort keys, and owned rows." Fixture: `accounts` (admin, hidden `password_hash`) and `tasks` (user, owner `owner_id` → typed `AccountId`) in `0001_notes.sql`; other tables are explicitly `public`. Owned + `admin` access compiles in principle (`admin.0`) but isn't in the fixture.
