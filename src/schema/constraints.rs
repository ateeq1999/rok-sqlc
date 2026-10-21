//! Named constraints and unique keys of a table.

/// A single-column foreign key with its name: the declared one, or the one
/// PostgreSQL gives an unnamed constraint (`<table>_<column>_fkey`).
#[derive(Debug, Clone)]
pub struct ForeignKey {
    pub name: String,
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
    /// Whether this is the primary key.
    pub primary: bool,
}

/// A `CHECK` constraint, or another constraint the model keeps only by
/// name (a composite foreign key), with its name: the declared one, or the
/// one PostgreSQL gives an unnamed constraint (`<table>_<column>_check`
/// for a check naming one column, `<table>_check` for any other check,
/// `<table>_<columns>_fkey`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedConstraint {
    pub name: String,
    /// The columns (SQL names) it names, in order of first use.
    pub columns: Vec<String>,
    /// A check's expression as SQL text, its column names kept current
    /// through renames; `None` for a constraint that is no check.
    pub check: Option<String>,
}

/// Columns no two rows share, from a unique constraint, a unique index on
/// plain columns, or a unique index on plain columns and one `lower(<column>)`
/// entry (a case-insensitive key), as declared. Whether a partial index makes a unique key is decided
/// by [`UniqueKey::counts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UniqueKey {
    /// Key columns in declaration order.
    pub columns: Vec<String>,
    /// The `WHERE` condition of a partial unique index, as SQL text; `None`
    /// for a constraint or a full index.
    pub predicate: Option<String>,
    /// The column of the key's `lower(<column>)` entry, when it has one: that
    /// column's values are compared ignoring case, so a lookup compares
    /// `lower(<column>) = lower($n)`, and the others as they are.
    pub lowered: Option<String>,
}

impl UniqueKey {
    /// A unique key with no partial-index condition.
    pub fn new(columns: Vec<String>) -> Self {
        Self {
            columns,
            predicate: None,
            lowered: None,
        }
    }

    /// Whether the key compares a column ignoring case (see
    /// [`lowered`](Self::lowered)).
    pub fn is_case_insensitive(&self) -> bool {
        self.lowered.is_some()
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
            lowered: None,
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
