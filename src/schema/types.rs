use heck::ToUpperCamelCase;

use super::rust_type::{BaseType, TYPES_MODULE};
use crate::IdGeneration;

#[derive(Debug, Clone)]
pub struct Schema {
    pub tables: Vec<Table>,
    /// Enum types in definition order, generated into the shared `types` module.
    pub enums: Vec<EnumType>,
}

/// A `CREATE TYPE … AS ENUM` type, after every `ALTER TYPE` applied to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumType {
    /// The schema qualifier as written (`app` in `app.status`); `None` when
    /// unqualified or qualified with `public`.
    pub schema: Option<String>,
    pub sql_name: String,
    /// The Rust enum name in the `types` module.
    pub rust_name: String,
    /// Variants in the database's sort order.
    pub variants: Vec<EnumVariant>,
    /// For a value-list column's enum, the column it restricts as written in
    /// DDL (`catalog.products.product_type`); such an enum encodes as text.
    /// `None` for a database enum type.
    pub column: Option<String>,
}

impl EnumType {
    /// The type name as written in DDL, e.g. `app.status`.
    pub fn display_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.sql_name),
            None => self.sql_name.clone(),
        }
    }

    /// The Rust type of a column of this enum, e.g. `types::Status`.
    pub fn rust_path(&self) -> String {
        format!("{TYPES_MODULE}::{}", self.rust_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    /// The exact database label.
    pub label: String,
    pub rust_name: String,
}

#[derive(Debug, Clone)]
pub struct Table {
    /// The schema qualifier as written in the DDL (`catalog` in
    /// `catalog.products`); `None` when the table name is unqualified.
    pub schema: Option<String>,
    pub sql_name: String,
    pub module_name: String,
    pub struct_name: String,
    pub columns: Vec<Column>,
    /// Primary key columns in declaration order; empty when the table has no primary key.
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKey>,
    /// Unique constraints and unique indexes on plain columns, in the order
    /// the migrations declare them; the primary key is not repeated here.
    pub unique_keys: Vec<UniqueKey>,
    /// The primary key and unique constraints by name, in declaration order:
    /// the names `ON CONFLICT ON CONSTRAINT` can use. A unique index is not a
    /// constraint, so it is not listed.
    pub key_constraints: Vec<KeyConstraint>,
    /// The version column (ADR 0010), as its SQL name: set only when the
    /// version convention is configured and this table has the column. The
    /// parser leaves it `None`; the generator applies the conventions.
    pub version_column: Option<String>,
    /// The soft-delete column (ADR 0010), when the soft-delete convention is
    /// configured and this table has that column. The parser leaves it
    /// `None`; the generator applies the conventions.
    pub soft_delete_column: Option<String>,
    /// The tenant column (ADR 0010), when the tenant convention is
    /// configured and this table has that column. The parser leaves it
    /// `None`; the generator applies the conventions.
    pub tenant_column: Option<String>,
    /// The relationships inferred from foreign keys (ADR 0014): belongs-to
    /// first, in foreign key order, then the inverses, in table order. The
    /// parser leaves it empty; the generator infers it over the tables
    /// generated together.
    pub relationships: Vec<Relationship>,
    /// The names of the row-level `BEFORE INSERT` triggers on this table
    /// (`BEFORE INSERT OR UPDATE` included), in creation order, less any
    /// dropped since. Such a trigger can supply a managed column's value;
    /// its function body is not inspected.
    pub before_insert_triggers: Vec<String>,
}

/// A relationship from a table to another table, or to itself, inferred from
/// one single-column foreign key (ADR 0014). Both tables are generated in the
/// same run, and the two columns it joins on have the same type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    /// The relationship's name, unique among the table's relationships and a
    /// valid Rust identifier: `category`, `products`,
    /// `invoices_by_created_by_user`, `parent`, `children`.
    pub name: String,
    pub kind: RelationshipKind,
    /// The related table's module name; module names are unique among the
    /// tables generated together.
    pub target: String,
    /// The column of this table's rows the relationship joins on (SQL name):
    /// the foreign key column for a belongs-to, the referenced column for a
    /// has-many or has-one.
    pub column: String,
    /// The column of the related table's rows equal to `column` (SQL name).
    pub target_column: String,
    /// The join table a many-to-many relationship goes through; `None` for
    /// every other kind.
    pub through: Option<Through>,
}

/// The join table of a many-to-many [`Relationship`]: each of its rows pairs
/// a row of the relationship's table with a row of the related table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Through {
    /// The join table's module name.
    pub table: String,
    /// The join table's column equal to the relationship's `column` (SQL name).
    pub column: String,
    /// The join table's column equal to the relationship's `target_column`
    /// (SQL name).
    pub target_column: String,
}

/// Which way a [`Relationship`] runs, and how many rows it reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelationshipKind {
    /// This table holds the foreign key; at most one related row.
    BelongsTo,
    /// The related table holds the foreign key; any number of related rows.
    HasMany,
    /// The related table holds the foreign key, and that column alone is a
    /// unique key there; at most one related row.
    HasOne,
    /// A join table pairs this table's rows with the related table's rows
    /// (see [`Relationship::through`]); any number of related rows.
    ManyToMany,
}

impl RelationshipKind {
    /// Whether one row relates to any number of rows.
    pub fn is_to_many(self) -> bool {
        matches!(
            self,
            RelationshipKind::HasMany | RelationshipKind::ManyToMany
        )
    }
}

impl Table {
    /// The table name as a human reads it in DDL, e.g. `catalog.products`.
    pub fn display_name(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{schema}.{}", self.sql_name),
            None => self.sql_name.clone(),
        }
    }

    /// The table's unique keys as column lists: the primary key first, then
    /// each unique constraint or index that [counts](UniqueKey::counts), in
    /// declaration order. A key over the same columns as an earlier one (in
    /// any order) is left out.
    pub fn unique_key_columns(&self, soft_delete_column: Option<&str>) -> Vec<Vec<String>> {
        let mut keys: Vec<Vec<String>> = Vec::new();
        if !self.primary_key.is_empty() {
            keys.push(self.primary_key.clone());
        }
        let declared = self
            .unique_keys
            .iter()
            .filter(|key| key.counts(soft_delete_column));
        for key in declared {
            let same = |other: &Vec<String>| {
                other.len() == key.columns.len()
                    && key.columns.iter().all(|column| other.contains(column))
            };
            if !keys.iter().any(same) {
                keys.push(key.columns.clone());
            }
        }
        keys
    }

    /// Primary key columns in key declaration order.
    pub fn primary_key_columns(&self) -> Vec<&Column> {
        self.primary_key
            .iter()
            .filter_map(|key| self.columns.iter().find(|column| column.sql_name == *key))
            .collect()
    }

    /// The primary key columns a caller passes to identify a row: every key
    /// column but the tenant column, which the tenant parameter supplies.
    /// Without a tenant column, the primary key columns.
    pub fn key_columns(&self) -> Vec<&Column> {
        self.primary_key_columns()
            .into_iter()
            .filter(|column| !self.is_tenant_column(column))
            .collect()
    }

    /// The tenant column, when the table has one.
    pub fn tenant(&self) -> Option<&Column> {
        let name = self.tenant_column.as_deref()?;
        self.columns.iter().find(|column| column.sql_name == name)
    }

    /// Whether `column` is this table's soft-delete column.
    pub fn is_soft_delete_column(&self, column: &Column) -> bool {
        self.soft_delete_column.as_deref() == Some(column.sql_name.as_str())
    }

    /// Whether `column` is this table's tenant column.
    pub fn is_tenant_column(&self, column: &Column) -> bool {
        self.tenant_column.as_deref() == Some(column.sql_name.as_str())
    }

    /// Whether a caller never supplies `column` in an input or patch: the
    /// soft-delete column (only `delete` and `restore` set it) and the tenant
    /// column (set from the tenant parameter, never changed).
    fn is_convention_controlled(&self, column: &Column) -> bool {
        self.is_soft_delete_column(column) || self.is_tenant_column(column)
    }

    /// Columns a caller may supply on insert. The soft-delete and tenant
    /// columns are left out.
    pub fn insert_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_insertable() && !self.is_convention_controlled(column))
            .collect()
    }

    /// Columns a caller may change on update. The soft-delete and tenant
    /// columns are left out.
    pub fn update_columns(&self) -> Vec<&Column> {
        self.columns
            .iter()
            .filter(|column| column.is_updatable() && !self.is_convention_controlled(column))
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct Column {
    pub sql_name: String,
    pub rust_name: String,
    pub rust_type: String,
    pub sqlx_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
    /// The database can supply a value (DEFAULT clause, serial, or identity).
    pub has_default: bool,
    /// The database always owns the value (serial, identity, or GENERATED);
    /// the column is never inserted or updated by generated code.
    pub is_generated: bool,
    /// The values a `CHECK (col IN ('a', ...))` restricts this text column to,
    /// deduplicated in list order. The parser turns such a column into a
    /// value-list enum; `None` for other columns.
    pub value_list: Option<Vec<String>>,
    /// The length, in characters, a `VARCHAR(n)` or `CHAR(n)` declares (1
    /// for a bare `CHAR`). `None` for other types and for `VARCHAR` without
    /// a length.
    pub declared_length: Option<u32>,
    /// The values single-column `CHECK`s against literals allow this numeric
    /// column (`col > 0`, `col BETWEEN 1 AND 9`, …), the tightest bounds
    /// when several apply. `None` when no such check bounds it.
    pub value_range: Option<ValueRange>,
    /// The lengths, in characters, `CHECK`s such as `char_length(col) <= 80`
    /// allow this text column, the tightest bounds when several apply.
    /// `None` when no such check bounds it.
    pub length_range: Option<LengthRange>,
    /// The typed id the generated code uses in place of `sqlx_type`: the
    /// table's own id for a single-column primary key, or the referenced
    /// table's id for a foreign key to it. The parser leaves it `None`; the
    /// generator fills it in for the tables it generates.
    pub typed_id: Option<TypedId>,
    /// A managed column (ADR 0010): the database maintains it, so it is
    /// never inserted or updated by generated code but stays in rows. The
    /// parser leaves it `false`; the generator applies the conventions.
    pub is_managed: bool,
    /// How generated code makes a new value for this column, when it is the
    /// table's one key column and
    /// [`Conventions::primary_key_ids`](crate::Conventions::primary_key_ids)
    /// applies to its type (a generated id). The
    /// parser leaves it `None`; the generator applies the conventions.
    pub id_generation: Option<IdGeneration>,
}

/// The values a column's `CHECK`s allow. At least one bound is set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueRange {
    pub min: Option<NumberBound>,
    pub max: Option<NumberBound>,
}

/// One end of a [`ValueRange`]: a number literal, written as SQL writes it
/// (`-5`, `0.25`), that fits the column's type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberBound {
    pub value: String,
    /// The bound itself is not allowed (`col > 0.5`). Bounds on integer
    /// columns are always inclusive.
    pub exclusive: bool,
}

/// The lengths, in characters, a column's `CHECK`s allow, both ends
/// inclusive. At least one bound is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LengthRange {
    pub min: Option<u32>,
    pub max: Option<u32>,
}

/// A generated typed id: `name` is defined in `<module>/models.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedId {
    pub module: String,
    pub name: String,
}

/// How a column can be filtered by the generated `search` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    /// Numbers, decimals, dates, and times: comparisons, ranges, and sets.
    Ordered,
    /// Text: equality, sets, and literal substring matching.
    Text,
    /// Booleans, UUIDs, enums, byte strings, and the types SQLx gives no
    /// `PartialOrd` (`INTERVAL`, `TIME WITH TIME ZONE`): equality and sets.
    Equality,
}

impl Column {
    /// How the column can be filtered, or `None` for arrays and JSON, which
    /// get only a null check.
    pub fn filter_kind(&self) -> Option<FilterKind> {
        match self.base_type() {
            BaseType::Integer
            | BaseType::Float
            | BaseType::Decimal
            | BaseType::Date
            | BaseType::Time
            | BaseType::Timestamp
            | BaseType::TimestampTz => Some(FilterKind::Ordered),
            BaseType::Text => Some(FilterKind::Text),
            BaseType::Bool
            | BaseType::Uuid
            | BaseType::Enum
            | BaseType::Interval
            | BaseType::Bytes
            | BaseType::TimeTz
            | BaseType::Money
            | BaseType::Range
            | BaseType::IpNetwork
            | BaseType::MacAddress => Some(FilterKind::Equality),
            BaseType::Array | BaseType::Other => None,
        }
    }

    /// What the column's base type (`sqlx_type`) is.
    pub fn base_type(&self) -> BaseType {
        BaseType::of(&self.sqlx_type)
    }

    /// The `XSortKey` variant naming this column.
    pub fn sort_variant(&self) -> String {
        let camel = self.rust_name.trim_matches('_').to_upper_camel_case();
        if camel.starts_with(|character: char| character.is_ascii_digit()) {
            format!("Column{camel}")
        } else {
            camel
        }
    }

    /// The `XFilter` field selecting rows where this column is or is not
    /// `NULL` (`note_is_null`; `type_` gives `type_is_null`).
    pub fn null_check_field(&self) -> String {
        format!("{}_is_null", self.rust_name.trim_end_matches('_'))
    }

    /// The caller may supply this column on insert: it is neither generated
    /// nor managed.
    pub fn is_insertable(&self) -> bool {
        !self.is_generated && !self.is_managed
    }

    /// The create input may leave this column out (`None`): the database
    /// default applies, or for a generated id, generated code makes one.
    pub fn is_optional_in_input(&self) -> bool {
        self.is_omittable_on_insert() || self.id_generation.is_some()
    }

    /// The caller may omit this column on insert so the database default
    /// applies. A generated id ([`id_generation`](Self::id_generation)) is
    /// always inserted: generated code makes the value the caller omits.
    pub fn is_omittable_on_insert(&self) -> bool {
        self.is_insertable() && self.has_default && self.id_generation.is_none()
    }

    /// The expression making a new value of this column's plain type
    /// (`sqlx_type`), when it has an [`id_generation`](Self::id_generation).
    pub fn new_id_expr(&self) -> Option<&'static str> {
        let text = self.base_type() == BaseType::Text;
        Some(match (self.id_generation?, text) {
            (IdGeneration::UuidV4, _) => "uuid::Uuid::new_v4()",
            (IdGeneration::UuidV7, _) => "uuid::Uuid::now_v7()",
            (IdGeneration::Ulid, false) => "uuid::Uuid::from(ulid::Ulid::new())",
            (IdGeneration::Ulid, true) => "ulid::Ulid::new().to_string()",
            (IdGeneration::Cuid2, true) => "cuid::cuid2()",
            // `primary_key_ids` marks only text keys for CUID2s.
            (IdGeneration::Cuid2, false) => return None,
        })
    }

    /// The caller may change this column on update: it is neither generated,
    /// managed, nor part of the primary key.
    pub fn is_updatable(&self) -> bool {
        !self.is_generated && !self.is_managed && !self.is_primary_key
    }

    /// The Rust type of one non-null value: the typed id when the column has
    /// one, otherwise `sqlx_type`.
    pub fn value_type(&self) -> &str {
        match &self.typed_id {
            Some(id) => &id.name,
            None => &self.sqlx_type,
        }
    }

    /// The Rust type of the column's field: `value_type`, wrapped in
    /// `Option` when the column is nullable.
    pub fn field_type(&self) -> String {
        if self.nullable {
            format!("Option<{}>", self.value_type())
        } else {
            self.value_type().to_owned()
        }
    }

    /// Whether a typed id can wrap this column's type: the type must be
    /// `Eq`, `Ord`, `Hash`, and `Display`.
    pub fn supports_typed_id(&self) -> bool {
        self.base_type().supports_typed_id()
    }

    /// Whether the column's value type is `Copy`.
    pub fn is_copy(&self) -> bool {
        self.supports_typed_id() && self.base_type() != BaseType::Text
    }
}

#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub column: String,
    /// The referenced table's schema qualifier as written; `None` when the
    /// reference is unqualified.
    pub referenced_schema: Option<String>,
    pub referenced_table: String,
    pub referenced_column: String,
}

/// A primary key or unique constraint with its name: the declared one, or
/// the one PostgreSQL gives an unnamed constraint (`<table>_pkey`,
/// `<table>_<columns>_key`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyConstraint {
    pub name: String,
    /// The constrained columns (SQL names), in declaration order.
    pub columns: Vec<String>,
}

/// Columns no two rows share, from a unique constraint or a unique index on
/// plain columns, as declared. Whether a partial index makes a unique key is
/// decided by [`UniqueKey::counts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueKey {
    /// Key columns in declaration order.
    pub columns: Vec<String>,
    /// The `WHERE` condition of a partial unique index, as SQL text; `None`
    /// for a constraint or a full index.
    pub predicate: Option<String>,
}

impl UniqueKey {
    /// A unique key with no partial-index condition.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            predicate: None,
        }
    }

    /// Whether the declaration makes a unique key: a constraint or a full
    /// index always does; a partial index only when its condition is exactly
    /// `<soft_delete_column> IS NULL`, so the key holds across live rows.
    pub fn counts(&self, soft_delete_column: Option<&str>) -> bool {
        let Some(predicate) = &self.predicate else {
            return true;
        };
        let Some(column) = soft_delete_column else {
            return false;
        };
        let mut condition = predicate.trim();
        while let Some(inner) = condition
            .strip_prefix('(')
            .and_then(|rest| rest.strip_suffix(')'))
        {
            condition = inner.trim();
        }
        let Some(subject) = condition
            .strip_suffix("IS NULL")
            .or_else(|| condition.strip_suffix("is null"))
        else {
            return false;
        };
        let subject = subject.trim();
        subject == column || subject == format!("\"{}\"", column.replace('"', "\"\""))
    }
}

#[cfg(test)]
mod tests {
    use super::UniqueKey;

    fn partial(predicate: &str) -> UniqueKey {
        UniqueKey {
            columns: vec!["sku".to_owned()],
            predicate: Some(predicate.to_owned()),
        }
    }

    #[test]
    fn a_partial_index_counts_only_on_exactly_the_soft_delete_condition() {
        let soft_delete = Some("deleted_at");
        assert!(UniqueKey::new(vec!["sku".to_owned()]).counts(None));
        assert!(partial("deleted_at IS NULL").counts(soft_delete));
        assert!(partial("(deleted_at IS NULL)").counts(soft_delete));
        assert!(!partial("deleted_at IS NULL").counts(None));
        assert!(!partial("archived_at IS NULL").counts(soft_delete));
        assert!(!partial("deleted_at IS NOT NULL").counts(soft_delete));
        assert!(!partial("deleted_at IS NULL AND sku <> ''").counts(soft_delete));
    }
}
