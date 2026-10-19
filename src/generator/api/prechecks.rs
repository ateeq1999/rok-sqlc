//! The pre-checks a generated write runs on its transaction before it
//! writes, each failure a field's: [`Reference`], a foreign key of the
//! create or update body, which must name a row the caller can see
//! (`exists`), then the body's unique keys, which no other row may hold
//! ([`super::unique`]). The failures collect in `shared/validation.rs`'s
//! `FieldFailures`, answered as one `422 validation_failed`; the
//! database's constraint stays the race's fallback (`409`).

use heck::ToSnakeCase;

use super::dto::{create_columns, update_columns};
use super::feature::{Bodies, Endpoint, Feature};
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
    /// Which bodies hold the column.
    pub bodies: Bodies,
}

impl Reference<'_> {
    /// The referenced row's singular name as words, as a message names it:
    /// "label", "audit log entry".
    pub fn singular(&self) -> String {
        self.target.struct_name.to_snake_case().replace('_', " ")
    }
}

/// Which of `feature`'s bodies hold every one of `columns`.
pub fn bodies_holding(feature: &Feature<'_>, columns: &[&Column]) -> Bodies {
    let holds = |body: &[&Column]| {
        columns
            .iter()
            .all(|column| body.iter().any(|c| c.sql_name == column.sql_name))
    };
    Bodies {
        create: feature.serves(Endpoint::Create) && holds(&create_columns(feature)),
        update: feature.serves(Endpoint::Update)
            && !feature.table.key_columns().is_empty()
            && holds(&update_columns(feature)),
    }
}

/// The references of `feature`'s bodies to tables among `tables`, checked
/// as `features` scope them. Only a foreign key to a whole single-column
/// key, of the key's own type, is checked. A reference to a tenant-scoped
/// table takes the caller's tenant: the feature's, or, from a feature
/// without a tenant column, the `Tenant` the request carries (the first
/// such table's type; one of another type isn't checked). A reference to
/// an owned table takes the caller when the caller is known (see
/// [`Feature::knows_caller`]). Others keep the constraint's `409`.
pub fn references<'a>(
    feature: &Feature<'a>,
    features: &[Feature<'a>],
    tables: &'a [Table],
) -> Vec<Reference<'a>> {
    let mut tenant: Option<&Column> = feature.tenant;
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
        let bodies = bodies_holding(feature, &[column]);
        if !typed || !bodies.any() {
            continue;
        }
        if let Some(theirs) = target.tenant() {
            match tenant {
                Some(ours) if ours.value_type() != theirs.value_type() => continue,
                Some(_) => {}
                None if theirs.is_copy() => tenant = Some(theirs),
                None => continue,
            }
        }
        let owned = feature.knows_caller()
            && features
                .iter()
                .any(|other| std::ptr::eq(other.table, target) && other.reads_owned_rows());
        references.push(Reference {
            column,
            target,
            owned,
            bodies,
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
        .filter(|reference| reference.bodies.holds(endpoint))
        .collect()
}

/// Whether `endpoint`'s `<write>_in` runs any pre-check.
fn runs_any(feature: &Feature<'_>, endpoint: Endpoint) -> bool {
    !checked(feature, endpoint).is_empty()
        || feature
            .unique_checks
            .iter()
            .any(|check| check.bodies.holds(endpoint))
}

/// Imports what [`push_checks`] renders for `endpoint` names: the
/// collector, and each referenced table's data layer module.
pub fn import(uses: &mut Uses, feature: &Feature<'_>, endpoint: Endpoint) {
    if runs_any(feature, endpoint) {
        uses.add(FIELD_FAILURES);
    }
    for reference in checked(feature, endpoint) {
        uses.add(&format!(
            "{}::{}",
            feature.db_path, reference.target.module_name
        ));
    }
}

/// The pre-checks `endpoint`'s `<write>_in` runs before it writes, on
/// `conn`: each reference of its body that is present and not `null` must
/// name a row the caller can see, each unique key it holds must be free,
/// and every failure is answered at once. Nothing when the body has
/// neither.
pub fn push_checks(output: &mut String, feature: &Feature<'_>, endpoint: Endpoint) {
    if !runs_any(feature, endpoint) {
        return;
    }
    output.push_str("        let mut failures = FieldFailures::default();\n");
    for reference in checked(feature, endpoint) {
        push_reference(output, endpoint, reference);
    }
    super::unique::push_checks(output, feature, endpoint);
    output.push_str("        failures.into_result()?;\n");
}

/// How a pre-check reads a column of a create or update body.
pub struct BodyField {
    /// The field, borrowed when the data layer takes it by reference:
    /// `input.title`, `&input.slug`.
    pub expr: String,
    /// How many `Option`s wrap it: none for a required create field, one
    /// for a field that may be absent, two for one that may also be `null`.
    pub options: usize,
}

impl BodyField {
    /// `column` of `endpoint`'s body.
    pub fn of(column: &Column, endpoint: Endpoint) -> Self {
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
        BodyField {
            expr: format!("{borrow}input.{}", column.rust_name),
            options,
        }
    }

    /// The `let` binding the value to `binding` when the field is present
    /// and not `null`: `let Some(Some(value)) = input.x`.
    pub fn binding(&self, binding: &str) -> String {
        format!(
            "let {}{binding}{} = {}",
            "Some(".repeat(self.options),
            ")".repeat(self.options),
            self.expr
        )
    }
}

/// One reference's check: `if let Some(value) = input.<column> { ... }`
/// around it for a field that may be absent or `null`.
fn push_reference(output: &mut String, endpoint: Endpoint, reference: &Reference<'_>) {
    let column = reference.column;
    let field = BodyField::of(column, endpoint);
    let module = &reference.target.module_name;
    let mut args = vec!["&mut *conn".to_owned()];
    if reference.target.tenant().is_some() {
        args.push("tenant".to_owned());
    }
    let indent = if field.options == 0 {
        args.push(field.expr.clone());
        8
    } else {
        output.push_str(&format!("        if {} {{\n", field.binding("value")));
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
    if field.options > 0 {
        output.push_str("        }\n");
    }
}
