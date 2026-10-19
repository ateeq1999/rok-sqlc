//! The `unique` pre-check a generated write runs on its transaction before
//! it writes: a create or update body holding a value a unique key of the
//! table already has is a field failure (code `unique`), collected with the
//! `exists` ones ([`super::prechecks`]) into one `422 validation_failed`.
//! Each checked key has a `<columns>_taken` read in the data layer's
//! `listing.rs`; the database's constraint stays the race's fallback
//! (`409`).

use super::feature::{Bodies, Endpoint, Feature};
use super::layout::push_call;
use super::prechecks::{BodyField, bodies_holding};
use crate::generator::sql::{equals, ident, live_condition, table_ref, tenant_condition};
use crate::generator::{
    StaticQuery, keys, param_name, param_type, push_and_chain, push_doc, push_fn_head,
};
use crate::schema::Column;

/// The locals of a `<write>_in` a check's binding must not shadow.
const SERVICE_LOCALS: &[&str] = &[
    "conn", "tenant", "user", "input", "id", "key", "version", "failures", "taken",
];

/// A unique key of a feature's table, checked before a create or update
/// whose body holds every one of its columns.
#[derive(Debug, Clone)]
pub struct UniqueCheck<'a> {
    /// The key's columns, less the tenant column and the owner column: the
    /// check always stays in the caller's tenant, and takes the owner from
    /// the caller.
    pub columns: Vec<&'a Column>,
    /// The column a case-insensitive key compares ignoring case (its
    /// `lower(<column>)` entry).
    pub lowered: Option<&'a Column>,
    /// The feature's owner column, when the key includes it: the check
    /// takes its value from the caller (`user.id`).
    pub owner: Option<&'a Column>,
    /// Whether the key holds across live rows only (a partial index on
    /// `<soft-delete column> IS NULL`), so soft-deleted rows don't count.
    pub live_only: bool,
    /// Which bodies hold every column.
    pub bodies: Bodies,
}

impl UniqueCheck<'_> {
    /// The data layer's read: `email_taken`, `region_and_seat_taken`.
    pub fn function(&self) -> String {
        let names: Vec<&str> = self
            .columns
            .iter()
            .map(|column| column.rust_name.trim_end_matches('_'))
            .collect();
        format!("{}_taken", names.join("_and_"))
    }

    /// Whether `column` is compared ignoring case.
    fn ignores_case(&self, column: &Column) -> bool {
        self.lowered
            .is_some_and(|lowered| lowered.sql_name == column.sql_name)
    }
}

/// The unique keys of `feature`'s table that its create or update body
/// holds whole (the tenant and owner columns aside), in declaration order.
/// A key with a column the body doesn't carry, or that can't be bound by
/// value or borrow, keeps the constraint's `409`.
pub fn checks<'a>(feature: &Feature<'a>) -> Vec<UniqueCheck<'a>> {
    let table = feature.table;
    let is_owner = |column: &Column| {
        feature
            .owner
            .is_some_and(|owner| owner.sql_name == column.sql_name)
    };
    let mut checks: Vec<UniqueCheck<'a>> = Vec::new();
    for key in table.lookup_unique_keys(table.soft_delete_column.as_deref()) {
        let Some(columns) = key
            .columns
            .iter()
            .map(|name| table.columns.iter().find(|column| column.sql_name == *name))
            .collect::<Option<Vec<&Column>>>()
        else {
            continue;
        };
        let owner = columns.iter().copied().find(|column| is_owner(column));
        let columns: Vec<&Column> = columns
            .into_iter()
            .filter(|column| !table.is_tenant_column(column) && !is_owner(column))
            .collect();
        let bindable = |column: &&Column| {
            (column.is_copy() || param_type(column).starts_with('&'))
                && !["except", "owner"].contains(&param_name(column).as_str())
        };
        if columns.is_empty() || !columns.iter().all(bindable) {
            continue;
        }
        let lowered = key
            .lowered
            .as_deref()
            .and_then(|name| columns.iter().copied().find(|c| c.sql_name == name));
        let check = UniqueCheck {
            bodies: bodies_holding(feature, &columns),
            columns,
            lowered,
            owner,
            live_only: key.predicate.is_some(),
        };
        let named = checks
            .iter()
            .any(|other| other.function() == check.function());
        if check.bodies.any() && !named {
            checks.push(check);
        }
    }
    checks
}

/// The `<columns>_taken` reads of `checks`, for `listing.rs`.
pub fn render_reads(output: &mut String, feature: &Feature<'_>, checks: &[UniqueCheck<'_>]) {
    for check in checks {
        output.push('\n');
        render_read(output, feature, check);
    }
}

/// One key's read: whether a row other than `except` has the values.
fn render_read(output: &mut String, feature: &Feature<'_>, check: &UniqueCheck<'_>) {
    let table = feature.table;
    let keys = table.key_columns();
    let mut params = keys::tenant_params(table);
    let mut binds: Vec<String> = keys::tenant_bind(table).into_iter().collect();
    let mut conditions: Vec<String> = tenant_condition(table, None).into_iter().collect();
    for column in &check.columns {
        let name = param_name(column);
        params.push(format!("{name}: {}", param_type(column)));
        binds.push(name);
        conditions.push(equals(column, binds.len(), check.ignores_case(column)));
    }
    if let Some(owner) = check.owner {
        params.push("owner: uuid::Uuid".to_owned());
        binds.push("owner".to_owned());
        conditions.push(format!("{} = ${}", ident(&owner.sql_name), binds.len()));
    }
    if check.live_only {
        conditions.extend(live_condition(table, None));
    }
    if !keys.is_empty() {
        params.push(format!("except: Option<{}>", keys::key_param_type(table)));
        let names: Vec<String> = keys.iter().map(|key| ident(&key.sql_name)).collect();
        let first = binds.len() + 1;
        let placeholders: Vec<String> = (first..first + keys.len())
            .map(|index| format!("${index}"))
            .collect();
        conditions.push(if let [name] = names.as_slice() {
            format!("{name} IS DISTINCT FROM {}", placeholders.join(", "))
        } else {
            format!(
                "({}) IS DISTINCT FROM ({})",
                names.join(", "),
                placeholders.join(", ")
            )
        });
        if keys.len() == 1 {
            binds.push("except".to_owned());
        } else {
            binds.extend(keys.iter().map(|key| {
                let borrow = if key.is_copy() { "" } else { "&" };
                format!("except.map(|key| {borrow}key.{})", key.rust_name)
            }));
        }
    }
    let mut sql = vec![
        "SELECT EXISTS (".to_owned(),
        "    SELECT 1".to_owned(),
        format!("    FROM {}", table_ref(table)),
    ];
    for (index, condition) in conditions.iter().enumerate() {
        let lead = if index == 0 { "WHERE" } else { "    AND" };
        sql.push(format!("    {lead} {condition}"));
    }
    sql.push(")".to_owned());

    let names: Vec<String> = check
        .columns
        .iter()
        .map(|column| format!("`{}`", column.sql_name))
        .collect();
    let ignoring_case = match check.lowered {
        Some(lowered) if check.columns.len() > 1 => {
            format!(", `{}` ignoring case", lowered.sql_name)
        }
        Some(_) => ", ignoring case".to_owned(),
        None => String::new(),
    };
    let mut doc = vec![format!(
        "Returns whether a row {}has this {}{ignoring_case}{}:",
        if keys.is_empty() {
            ""
        } else {
            "other than `except` "
        },
        join_words(&names),
        if check.owner.is_some() {
            " among `owner`'s rows"
        } else {
            ""
        },
    )];
    doc.push("the `unique` check of a create or update body.".to_owned());
    if check.live_only {
        doc.push("Soft-deleted rows are left out.".to_owned());
    }
    let doc: Vec<&str> = doc.iter().map(String::as_str).collect();
    push_doc(output, 0, &doc);
    push_fn_head(
        output,
        &check.function(),
        &params,
        "Result<bool, sqlx::Error>",
    );
    StaticQuery {
        call: "sqlx::query_scalar::<_, bool>",
        sql,
        binds,
        execute: "fetch_one(executor)",
    }
    .push(output, 4, "", "");
    output.push_str("}\n");
}

/// `a`, `a and b`, `a, b and c`.
fn join_words(words: &[String]) -> String {
    match words {
        [] => String::new(),
        [word] => word.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// The checks `endpoint`'s `<write>_in` runs, on `conn`, adding to
/// `failures`: each key whose values its body holds, present and not
/// `null`, must not be taken by another row (on an update, another than
/// the row with the key).
pub fn push_checks(output: &mut String, feature: &Feature<'_>, endpoint: Endpoint) {
    for check in feature
        .unique_checks
        .iter()
        .filter(|check| check.bodies.holds(endpoint))
    {
        push_check(output, feature, endpoint, check);
    }
}

/// One key's check, inside `if let Some(..) = ..` for each value that may
/// be absent or `null`.
fn push_check(
    output: &mut String,
    feature: &Feature<'_>,
    endpoint: Endpoint,
    check: &UniqueCheck<'_>,
) {
    let mut args = super::service::data_args(feature, "&mut *conn");
    let mut lets = Vec::new();
    for column in &check.columns {
        let field = BodyField::of(column, endpoint);
        if field.options == 0 {
            args.push(field.expr);
        } else {
            let mut binding = param_name(column);
            if SERVICE_LOCALS.contains(&binding.as_str()) {
                binding.push_str("_value");
            }
            lets.push(field.binding(&binding));
            args.push(binding);
        }
    }
    if check.owner.is_some() {
        args.push("user.id".to_owned());
    }
    let keys = feature.table.key_columns();
    if !keys.is_empty() {
        args.push(if endpoint == Endpoint::Update {
            format!("Some({})", super::service::key_arg(feature))
        } else {
            "None".to_owned()
        });
    }
    let indent = if lets.is_empty() { 8 } else { 12 };
    if let [single] = lets.as_slice() {
        push_and_chain(output, 8, "if ", std::slice::from_ref(single), " {");
    } else if let [first, rest @ ..] = lets.as_slice() {
        // rustfmt puts each `let` of a chain on a line of its own.
        output.push_str(&format!("        if {first}\n"));
        for term in rest {
            output.push_str(&format!("            && {term}\n"));
        }
        output.push_str("        {\n");
    }
    let pad = " ".repeat(indent);
    let callee = format!("repo::{}", check.function());
    push_call(output, indent, "let taken = ", &callee, &args, ".await?;");
    output.push_str(&format!("{pad}if taken {{\n"));
    let fields: Vec<String> = check
        .columns
        .iter()
        .map(|column| format!("\"{}\"", column.sql_name))
        .collect();
    if let [field] = fields.as_slice() {
        push_call(
            output,
            indent + 4,
            "",
            "failures.taken",
            std::slice::from_ref(field),
            ";",
        );
    } else {
        let inner = " ".repeat(indent + 4);
        let one_line = format!("{inner}failures.taken_together(&[{}]);", fields.join(", "));
        if one_line.len() <= crate::generator::MAX_WIDTH {
            output.push_str(&one_line);
            output.push('\n');
        } else {
            // rustfmt's layout: one name per line, the slice hugging the call.
            output.push_str(&format!("{inner}failures.taken_together(&[\n"));
            for field in &fields {
                output.push_str(&format!("{inner}    {field},\n"));
            }
            output.push_str(&format!("{inner}]);\n"));
        }
    }
    output.push_str(&format!("{pad}}}\n"));
    if !lets.is_empty() {
        output.push_str("        }\n");
    }
}
