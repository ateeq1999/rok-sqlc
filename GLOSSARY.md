# rok-sqlc

A CLI that reads PostgreSQL schema files and generates SQLx-based Rust modules.

## Language

**Table**:
A table declared by `CREATE TABLE` in the input schema; the input side of generation.
_Avoid_: entity, resource (when referring to the SQL side)

**Resource**:
The Rust module rok-sqlc generates for one table; the output side of generation.
_Avoid_: table (when referring to generated output)

**Schema snapshot**:
A verbatim copy of the input schema SQL embedded in generated code as a constant. Never executed; not a migration system.
_Avoid_: migration

**Module fixture**:
An ERP database schema under `modules/` used as a test input; each carries an explicit expected outcome.
_Avoid_: test schema, sample

**Query file**:
SQLC-annotated SQL authored alongside a module fixture (`modules/<module>/queries.sql`) describing application-specific statements: data manipulation, filtered reads, pagination, transactions.
_Avoid_: schema (for DML content), script

## Relationships

**Relationship**:
An association between two tables that generation may traverse to produce baseline join reads.
_Avoid_: join (the SQL mechanic), link

**Belongs-to**:
A relationship from the table holding the foreign key to the table it references.
_Avoid_: reference, parent

**Has-many**:
A relationship from a referenced table to the tables whose foreign keys point at it.
_Avoid_: children

**Has-one**:
A has-many variant where the foreign key carries a uniqueness constraint.
_Avoid_: single child

**Through relationship**:
A two-hop relationship traversing an intermediate table (has-many-through, has-one-through).
_Avoid_: nested

**Many-to-many**:
Two has-many-through relationships over a shared join table.
_Avoid_: m2m

**Join table**:
An intermediate table holding two foreign keys that implements a many-to-many.
_Avoid_: pivot, junction

**Polymorphic association**:
A declared (never inferred) association whose foreign key and type discriminator may point at one of several tables.
_Avoid_: FK (for this construct)
