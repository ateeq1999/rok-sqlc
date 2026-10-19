//! The checks a generated write runs on its transaction before it writes,
//! each failure a field's: [`Reference`], a foreign key of the create or
//! update body, which must name a row the caller can see (`exists`). The
//! failures collect in `shared/validation.rs`'s `FieldFailures`, answered
//! as one `422 validation_failed`; the database's constraint stays the
//! race's fallback (`409`).

use heck::ToSnakeCase;

use super::dto::{create_columns, update_columns};
use super::feature::{Endpoint, Feature};
use super::layout::{Uses, push_call};
use crate::generator::keys::referenced_table;
use crate::generator::{insert_field_type, param_type};
use crate::schema::{Column, Table};

/// The `use` path of the collector.
const FIELD_FAILURES: &str = "crate::shared::validation::FieldFailures";

/// A foreign key of a feature's create or update body, checked to name a
/// row the caller can see: one that exists, is live (soft delete), is in
/// the caller's tenant and, when its table's feature is owned and the
/// caller is known, is the caller's.
#[derive(Debug, Clone)]
pub struct Reference<'a> {
    /// The referencing column, a field of the body.
    pub column: &'a Column,
    /// The referenced table, whose single-column key the column names.
    pub target: &'a Table,
    /// Whether the check is the target feature's `exists_owned`, scoped to
    /// the caller (`user.id`); otherwise the data layer's `exists_by_id`.
    pub owned: bool,
    /// Whether the create body holds the column.
    pub on_create: bool,
    /// Whether the update body holds the column.
    pub on_update: bool,
}

impl Reference<'_> {
    /// The referenced row's singular name as words, as a message names it:
    /// "label", "audit log entry".
    pub fn singular(&self) -> String {
        self.target.struct_name.to_snake_case().replace('_', " ")
    }
}

/// The references of `feature`'s bodies to tables among `tables`, checked
/// as `features` scope them. Only a foreign key to a whole single-column
/// key, of the key's own type, is checked; and a reference to a
/// tenant-scoped table only from a tenant-scoped feature, which knows the
/// tenant. Others keep the constraint's `409`.
pub fn references<'a>(
    feature: &Feature<'a>,
    features: &[Feature<'a>],
    tables: &'a [Table],
) -> Vec<Reference<'a>> {
    let creates = feature.serves(Endpoint::Create);
    let updates = feature.serves(Endpoint::Update);
    let create = create_columns(feature);
    let update = update_columns(feature);
    let mut references = Vec::new();
    for foreign_key in &feature.table.foreign_keys {
        let Some(column) = feature
            .table
            .columns
            .iter()
            .find(|column| column.sql_name == foreign_key.column)
        else {
            continue;
        };
        let Some(target) = referenced_table(tables, foreign_key) else {
            continue;
        };
        let keys = target.key_columns();
        let [key] = keys.as_slice() else {
            continue;
        };
        let typed = key.sql_name == foreign_key.referenced_column
            && key.value_type() == column.value_type()
            && (column.is_copy() || param_type(column).starts_with('&'));
        let tenant = match (target.tenant(), feature.tenant) {
            (None, _) => true,
            (Some(theirs), Some(ours)) => theirs.value_type() == ours.value_type(),
            (Some(_), None) => false,
        };
        let has = |columns: &[&Column]| columns.iter().any(|c| c.sql_name == column.sql_name);
        let on_create = creates && has(&create);
        let on_update = updates && has(&update);
        if !typed || !tenant || !(on_create || on_update) {
            continue;
        }
        let owned = feature.owner.is_some()
            && features
                .iter()
                .any(|other| std::ptr::eq(other.table, target) && other.reads_owned_rows());
        references.push(Reference {
            column,
            target,
            owned,
            on_create,
            on_update,
        });
    }
    references
}

/// The references `endpoint`'s body holds: none but a create's or an
/// update's.
fn checked<'f, 'a>(feature: &'f Feature<'a>, endpoint: Endpoint) -> Vec<&'f Reference<'a>> {
    feature
        .references
        .iter()
        .filter(|reference| match endpoint {
            Endpoint::Create => reference.on_create,
            Endpoint::Update => reference.on_update,
            _ => false,
        })
        .collect()
}

/// Imports what [`push_checks`] renders for `endpoint` names: the
/// collector, and each referenced table's data layer module.
pub fn import(uses: &mut Uses, feature: &Feature<'_>, endpoint: Endpoint) {
    let checked = checked(feature, endpoint);
    if !checked.is_empty() {
        uses.add(FIELD_FAILURES);
    }
    for reference in checked {
        uses.add(&format!(
            "{}::{}",
            feature.db_path, reference.target.module_name
        ));
    }
}

/// The checks `endpoint`'s `<write>_in` runs before it writes, on `conn`:
/// each reference of its body that is present and not `null` must name a
/// row the caller can see, and every failure is answered at once. Nothing
/// when the body has no reference.
pub fn push_checks(output: &mut String, feature: &Feature<'_>, endpoint: Endpoint) {
    let checked = checked(feature, endpoint);
    if checked.is_empty() {
        return;
    }
    output.push_str("        let mut failures = FieldFailures::default();\n");
    for reference in checked {
        push_reference(output, endpoint, reference);
    }
    output.push_str("        failures.into_result()?;\n");
}

/// One reference's check: `if let Some(value) = input.<column> { ... }`
/// around it for a field that may be absent or `null`.
fn push_reference(output: &mut String, endpoint: Endpoint, reference: &Reference<'_>) {
    let column = reference.column;
    let options = if endpoint == Endpoint::Create {
        insert_field_type(column).matches("Option<").count()
    } else {
        1 + usize::from(column.nullable)
    };
    let borrow = if param_type(column).starts_with('&') {
        "&"
    } else {
        ""
    };
    let field = format!("{borrow}input.{}", column.rust_name);
    let module = &reference.target.module_name;
    let mut args = vec!["&mut *conn".to_owned()];
    if reference.target.tenant().is_some() {
        args.push("tenant".to_owned());
    }
    let indent = if options == 0 {
        args.push(field);
        8
    } else {
        let pattern = format!("{}value{}", "Some(".repeat(options), ")".repeat(options));
        output.push_str(&format!("        if let {pattern} = {field} {{\n"));
        args.push("value".to_owned());
        12
    };
    let callee = if reference.owned {
        args.push("user.id".to_owned());
        format!("{module}::listing::exists_owned")
    } else {
        format!("{module}::read::exists_by_id")
    };
    let pad = " ".repeat(indent);
    push_call(output, indent, "let exists = ", &callee, &args, ".await?;");
    output.push_str(&format!("{pad}if !exists {{\n"));
    let missing = [
        format!("\"{}\"", column.sql_name),
        format!("\"{}\"", reference.singular()),
    ];
    push_call(output, indent + 4, "", "failures.missing", &missing, ";");
    output.push_str(&format!("{pad}}}\n"));
    if options > 0 {
        output.push_str("        }\n");
    }
}
