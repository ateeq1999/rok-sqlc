# 07: Access, ownership, and hidden columns

**What to build:** The access part of ADR 0016's wire standard, on the features from ticket 06:

- **Access:** each table's handlers take `AuthUser` (`api.access` `user`, the default), `AdminUser` (`admin`), or neither (`public`). Both extractors come from the scaffolded `shared/auth.rs`.
- **Ownership** (`api.owner`): the table's queries are scoped to the caller's id (`WHERE … AND owner = $n`) in new or extended `db` functions. `create` sets the owner from `AuthUser` and takes no owner field in `CreateXDto`, and `UpdateXDto` can't change it. Another caller's row is a 404. The service trait's methods take the `AuthUser` where ownership or access needs it.
- **Hidden columns** (`api.hidden`): left out of `XResponse`. A hidden column stays in `CreateXDto` and `UpdateXDto` (the service may transform it, e.g. hash a password). The `sort` param can't name one (so it gets no `XSort` variant). If ticket 08's search endpoint is on, it's rejected there. Coordinate by leaving a clear hook.

**Blocked by:** 06

**Status:** ready-for-agent

- [ ] The `tests/api-app` fixture covers each access level (401 without a token, 403 for a non-admin), owner scoping (another user's row gives 404, create sets the owner), and that hidden columns never appear in a response.
- [ ] The lint gate passes.
