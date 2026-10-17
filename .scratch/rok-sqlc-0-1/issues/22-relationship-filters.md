# 22: Relationship filters in search

**What to build:** Each resource's search filter gains an optional field per relationship, holding the related resource's filter (for example, `ProductFilter { category: Some(CategoryFilter { name: … }) }`). It is rendered as an `EXISTS (…)` subquery so result rows are never duplicated. For to-many relationships it means "has at least one related row matching". Related filters may nest relationship filters, up to a fixed depth that is documented. Soft delete and tenant scoping apply inside the subquery.

**Blocked by:** 20

**Status:** resolved

- [x] Searching products by their category's name returns exactly the matching products, each once (PostgreSQL seam).
- [x] Searching posts that have at least one comment by a given author returns each post once, even with several matching comments.
- [x] Soft-deleted or other-tenant related rows never satisfy a relationship filter.
- [x] An empty related filter means "has a related row" (for to-many) or "has a non-null, live parent" (for to-one), as documented.

## Comments

Resolved on integration branch `rok-sqlc-0-1` (final tip b865371). Final code review fixes landed in 8dab99f..b865371. See CHANGELOG.md (Unreleased) and the ADRs for design decisions.
