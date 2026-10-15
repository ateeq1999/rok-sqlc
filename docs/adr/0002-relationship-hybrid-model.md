# Relationships: hybrid inference + declaration, baseline traversals only

Relationships are known to rok-sqlc two ways: single-hop relationships (belongs-to, has-many, has-one) are inferred from foreign keys parsed out of the DDL, while everything inference cannot see — polymorphic associations (no DDL footprint), through-relationship naming, opt-outs — is declared via configuration. Generation produces only the predictable baseline traversals (`get_with_author`, `list_comments_for_post`); every ad-hoc or multi-table join remains query-file territory, keeping the two-lane principle (schema → baseline, queries → app-specific) intact. Relationship generation lands as phase 9, after query files.

## Considered Options

- **Pure inference** — rejected: polymorphic associations have no FK representation and cannot be inferred.
- **Pure declaration** — rejected: boilerplate for the structural majority of relationships that FKs already express.
