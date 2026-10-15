# 19: Version column and `UpdateOutcome`

**What to build:** For tables with the configured **version column**, `update` takes the expected version and returns `Result<UpdateOutcome<Row>, sqlx::Error>`, where `UpdateOutcome` is a shared generated type with three cases: `Updated(row)`, `NotFound`, and `VersionConflict`. The version check is part of the update's condition. When nothing was updated, a follow-up existence check distinguishes not-found from a conflict, within the same executor call sequence. The version column itself is a managed column and is excluded from patches. An empty patch still returns the current row without bumping the version.

**Blocked by:** 16

**Status:** ready-for-agent

- [ ] Two updates made with the same expected version: the first returns `Updated` and the second returns `VersionConflict` (PostgreSQL seam, with a version-bumping trigger like `touch_row`).
- [ ] An update of a missing key returns `NotFound`.
- [ ] Combined with soft delete, updating a deleted row returns `NotFound`.
- [ ] Without the convention, `update` keeps its current signature.
