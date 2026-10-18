//! Emitting Rust source laid out as rustfmt lays it out: `use` lists, docs,
//! function heads, method chains, and derives.

use super::sql;
use crate::Derive;
use crate::schema::{Table, mentions_generated_type};

/// rustfmt's default `max_width`; generated code is laid out to be
/// rustfmt-stable so regenerated files diff cleanly.
pub(super) const MAX_WIDTH: usize = 100;

/// rustfmt's default `chain_width`: a method chain longer than this goes one
/// call per line.
pub(super) const CHAIN_WIDTH: usize = 60;

/// rustfmt's default `fn_call_width`: call arguments longer than this go one
/// per line.
pub(super) const FN_CALL_WIDTH: usize = 60;

/// Appends the method chain `{head}{calls}` at `indent`, after `lead` (such
/// as `"&& "`): on one line when the chain fits [`CHAIN_WIDTH`], otherwise
/// with `head` on the first line and each call on its own line below,
/// indented once more, as rustfmt splits it.
pub fn push_chain(output: &mut String, indent: usize, lead: &str, head: &str, calls: &[String]) {
    let pad = " ".repeat(indent);
    let chain = format!("{head}{}", calls.concat());
    if chain.len() <= CHAIN_WIDTH {
        output.push_str(&format!("{pad}{lead}{chain}\n"));
        return;
    }
    output.push_str(&format!("{pad}{lead}{head}\n"));
    for call in calls {
        output.push_str(&format!("{pad}    {call}\n"));
    }
}

/// The derives `derive = ["serde"]` adds (ADR 0013), for [`push_derives`]:
/// none without `serde`.
pub fn serde_derives(serde: bool) -> &'static [&'static str] {
    if serde {
        &["serde::Serialize", "serde::Deserialize"]
    } else {
        &[]
    }
}

/// What a run's `derive` asks of the generated items: serde's derives
/// (ADR 0013), and `validator::Validate` with its rules on create inputs and
/// update patches (ADR 0015), and with `[api] openapi`, utoipa's
/// `ToSchema` on the types an API feature exposes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeriveOptions {
    /// `derive = ["serde"]`.
    pub serde: bool,
    /// `derive = ["validate"]`.
    pub validate: bool,
    /// `api.output` with `[api] openapi` on: the typed ids and enums derive
    /// `utoipa::ToSchema`.
    pub openapi: bool,
}

impl DeriveOptions {
    /// The options `derive` lists, and `openapi`.
    pub fn of(derive: &[Derive], openapi: bool) -> Self {
        DeriveOptions {
            serde: derive.contains(&Derive::Serde),
            validate: derive.contains(&Derive::Validate),
            openapi,
        }
    }

    /// The derives of a typed id or an enum, which the API may expose:
    /// serde's, then `utoipa::ToSchema` with `openapi`.
    pub fn value_derives(self) -> Vec<&'static str> {
        let mut derives = serde_derives(self.serde).to_vec();
        derives.extend(super::openapi::to_schema(self.openapi));
        derives
    }

    /// The derives a create input or update patch adds: serde's, then
    /// `validator::Validate` with `validate`.
    pub fn input_derives(self) -> Vec<&'static str> {
        let mut derives = serde_derives(self.serde).to_vec();
        if self.validate {
            derives.push("validator::Validate");
        }
        derives
    }
}

/// `template` with the first `pattern` replaced by `with`. A template
/// written without `pattern` is a bug in the template or in its patch, so a
/// debug build panics instead of leaving the text silently unpatched; each
/// patched template has a unit test running its patches.
pub fn replace_once(template: &str, pattern: &str, with: &str) -> String {
    debug_assert!(
        template.contains(pattern),
        "the template no longer holds {pattern:?}"
    );
    template.replacen(pattern, with, 1)
}

/// Adds `extra` derives to the `#[derive(...)]` line `output` ends with, laid
/// out as rustfmt lays out a derive list: on one line when it fits,
/// otherwise one derive per line. No `extra` derive leaves `output` as is.
pub fn push_derives(output: &mut String, extra: &[&str]) {
    if extra.is_empty() {
        return;
    }
    let without_newline = output.strip_suffix('\n').unwrap_or(output);
    let start = without_newline.rfind('\n').map_or(0, |index| index + 1);
    let derives = output[start..]
        .trim_end()
        .strip_prefix("#[derive(")
        .and_then(|rest| rest.strip_suffix(")]"))
        .expect("derives follow a `#[derive(...)]` line")
        .to_owned();
    output.truncate(start);
    let derives = format!("{derives}, {}", extra.join(", "));
    // rustfmt keeps the list on one line only up to 96 columns, then on one
    // indented line up to 100, then one per line.
    let one_line = format!("#[derive({derives})]");
    let packed = format!("    {derives},");
    if one_line.len() <= MAX_WIDTH - 4 {
        output.push_str(&one_line);
        output.push('\n');
    } else if packed.len() <= MAX_WIDTH {
        output.push_str(&format!("#[derive(\n{packed}\n)]\n"));
    } else {
        output.push_str("#[derive(\n");
        for derive in derives.split(", ") {
            output.push_str(&format!("    {derive},\n"));
        }
        output.push_str(")]\n");
    }
}

/// Makes a serde-deriving search enum (a condition, `Direction`, or a sort
/// key) read and write its variants in snake_case, as JSON APIs expect
/// (`{"ge": 400}`, `"price_cents"`; ADR 0013). Generated enum types keep
/// their database labels instead.
pub fn push_serde_snake_case(output: &mut String) {
    output.push_str("#[serde(rename_all = \"snake_case\")]\n");
}

/// `use {prefix}::...;` for sorted `names`, wrapped like rustfmt wraps a use
/// list that does not fit on one line.
pub fn use_list(prefix: &str, names: &[String]) -> String {
    if let [name] = names {
        return format!("use {prefix}::{name};\n");
    }
    let one_line = format!("use {prefix}::{{{}}};", names.join(", "));
    // rustfmt keeps a use list on one line only up to 98 columns, and packs
    // the wrapped items greedily into lines of up to 99 columns.
    if one_line.len() < MAX_WIDTH - 1 {
        return format!("{one_line}\n");
    }
    let mut output = format!("use {prefix}::{{\n");
    let mut line = String::new();
    for name in names {
        if !line.is_empty() && line.len() + 1 + name.len() + 1 > MAX_WIDTH - 1 {
            output.push_str(&line);
            output.push('\n');
            line.clear();
        }
        if line.is_empty() {
            line.push_str(&format!("    {name},"));
        } else {
            line.push_str(&format!(" {name},"));
        }
    }
    output.push_str(&line);
    output.push_str("\n};\n");
    output
}

/// `use` lines for full paths (one item each), in rustfmt's order: `super`
/// segments before named modules.
pub fn use_paths(paths: &[String]) -> String {
    let mut sorted: Vec<&String> = paths.iter().collect();
    sorted.sort_by_key(|path| {
        path.split("::")
            .map(|segment| (segment != "super", segment.to_owned()))
            .collect::<Vec<_>>()
    });
    sorted.dedup();
    sorted.iter().map(|path| format!("use {path};\n")).collect()
}

/// `use {db_path}::types;` when any of the Rust types a file mentions is a
/// generated enum (`types::X`), else empty. `db_path` is the path from the
/// file's module to `db`, e.g. `super::super` from a resource file.
pub fn types_import<'a>(db_path: &str, rust_types: impl IntoIterator<Item = &'a str>) -> String {
    if rust_types.into_iter().any(mentions_generated_type) {
        format!("use {db_path}::types;\n")
    } else {
        String::new()
    }
}

/// A SQL name in backticks for a one-line doc comment: line breaks a
/// quoted identifier may hold become spaces.
pub fn doc_name(sql_name: &str) -> String {
    format!("`{}`", sql_name.replace(['\r', '\n'], " "))
}

/// Appends `/// line` doc comments at `indent`.
pub fn push_doc(output: &mut String, indent: usize, lines: &[&str]) {
    let pad = " ".repeat(indent);
    for line in lines {
        if line.is_empty() {
            output.push_str(&format!("{pad}///\n"));
        } else {
            output.push_str(&format!("{pad}/// {line}\n"));
        }
    }
}

/// Appends a generated query function's head: the signature (one line when
/// it fits, otherwise one parameter per line), the shared executor bound, and
/// the opening brace.
pub fn push_fn_head(output: &mut String, name: &str, params: &[String], returns: &str) {
    let mut all_params = vec!["executor: E".to_owned()];
    all_params.extend(params.iter().cloned());

    let open = format!("pub async fn {name}<'e, E>(");
    let close = format!(") -> {returns}");
    let one_line = format!("{open}{}{close}", all_params.join(", "));
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
    } else {
        output.push_str(&open);
        output.push('\n');
        for param in &all_params {
            output.push_str(&format!("    {param},\n"));
        }
        output.push_str(&close);
        output.push('\n');
    }
    output.push_str("where\n    E: sqlx::PgExecutor<'e>,\n{\n");
}

/// Renders `a && b && c` at `indent`, wrapped the way rustfmt wraps it.
/// `lead` and `trail` surround the chain (`"if "` and `" {"` for a
/// condition); a wrapped condition puts its opening brace on its own line.
pub fn push_and_chain(
    output: &mut String,
    indent: usize,
    lead: &str,
    terms: &[String],
    trail: &str,
) {
    let pad = " ".repeat(indent);
    let one_line = format!("{pad}{lead}{}{trail}", terms.join(" && "));
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("{pad}{lead}{}\n", terms[0]));
    for term in &terms[1..] {
        output.push_str(&format!("{pad}    && {term}\n"));
    }
    let trail = trail.trim_start();
    if !trail.is_empty() {
        output.push_str(&format!("{pad}{trail}\n"));
    }
}

/// Appends `{binding} = {call}({argument});` at `indent`, laid out the way
/// rustfmt lays it out: on one line when it fits, otherwise with the call
/// moved below the `=`, otherwise with the argument on its own line.
pub fn push_let_call(
    output: &mut String,
    indent: usize,
    binding: &str,
    call: &str,
    argument: &str,
) {
    let pad = " ".repeat(indent);
    let one_line = format!("{pad}{binding} = {call}({argument});");
    let below = format!("{pad}    {call}({argument});");
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
    } else if below.len() <= MAX_WIDTH {
        output.push_str(&format!("{pad}{binding} =\n{below}\n"));
    } else {
        output.push_str(&format!(
            "{pad}{binding} = {call}(\n{pad}    {argument},\n{pad});\n"
        ));
    }
}

/// A statement whose SQL is fully known at generation time.
pub struct StaticQuery<'a> {
    /// The SQLx entry point, e.g. `sqlx::query_as::<_, Product>`.
    pub call: &'a str,
    pub sql: Vec<String>,
    /// Bind expressions in placeholder order: the n-th entry binds `$n`.
    pub binds: Vec<String>,
    /// The execution method call, e.g. `fetch_optional(executor)`.
    pub execute: &'a str,
}

impl StaticQuery<'_> {
    /// Appends the query as a method chain at `indent`, preceded by `lead`
    /// (such as `let result = ` or `return `) and ending with `.await` plus
    /// `trail` (such as `?;`).
    pub fn push(&self, output: &mut String, indent: usize, lead: &str, trail: &str) {
        let pad = " ".repeat(indent);
        output.push_str(&format!("{pad}{lead}{}(\n", self.call));
        output.push_str(&sql::raw_block(&self.sql, indent + 4));
        output.push_str(",\n");
        output.push_str(&format!("{pad})\n"));
        for bind in &self.binds {
            output.push_str(&format!("{pad}.bind({bind})\n"));
        }
        output.push_str(&format!("{pad}.{}\n", self.execute));
        output.push_str(&format!("{pad}.await{trail}\n"));
    }
}

/// The final `build_query_as().fetch_*().await` chain, kept on one line when
/// it fits [`CHAIN_WIDTH`] and split per call otherwise.
pub fn push_build_and_fetch(output: &mut String, table: &Table, fetch: &str) {
    let calls = [
        format!(".build_query_as::<{}>()", table.struct_name),
        format!(".{fetch}(executor)"),
        ".await".to_owned(),
    ];
    push_chain(output, 4, "", "query", &calls);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// rustfmt merges adjacent derives, so serde joins the existing list,
    /// wrapped one per line once it is too wide.
    #[test]
    fn serde_derives_join_the_derive_list_as_rustfmt_lays_it_out() {
        let mut short = "/// Doc.\n#[derive(Debug, Clone)]\n".to_owned();
        push_derives(&mut short, serde_derives(true));
        assert_eq!(
            short,
            "/// Doc.\n#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]\n"
        );

        let mut long =
            "#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, sqlx::Type)]\n"
                .to_owned();
        push_derives(&mut long, serde_derives(true));
        assert_eq!(
            long,
            "#[derive(\n    Debug,\n    Clone,\n    Copy,\n    PartialEq,\n    Eq,\n    PartialOrd,\n    Ord,\n    Hash,\n    sqlx::Type,\n    serde::Serialize,\n    serde::Deserialize,\n)]\n"
        );

        let mut packed =
            "#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]\n".to_owned();
        push_derives(&mut packed, serde_derives(true));
        assert_eq!(
            packed,
            "#[derive(\n    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,\n)]\n"
        );

        let mut off = "#[derive(Debug)]\n".to_owned();
        push_derives(&mut off, serde_derives(false));
        assert_eq!(off, "#[derive(Debug)]\n");
    }

    #[test]
    fn fn_head_stays_on_one_line_when_it_fits() {
        let mut output = String::new();
        push_fn_head(&mut output, "list", &[], "Result<(), sqlx::Error>");
        assert_eq!(
            output,
            "pub async fn list<'e, E>(executor: E) -> Result<(), sqlx::Error>\nwhere\n    E: sqlx::PgExecutor<'e>,\n{\n"
        );
    }

    #[test]
    fn fn_head_goes_vertical_when_too_wide() {
        let mut output = String::new();
        let params = vec![
            "order_id: uuid::Uuid".to_owned(),
            "product_id: uuid::Uuid".to_owned(),
        ];
        push_fn_head(
            &mut output,
            "get_by_id",
            &params,
            "Result<Option<LineItem>, sqlx::Error>",
        );
        assert!(output.starts_with(
            "pub async fn get_by_id<'e, E>(\n    executor: E,\n    order_id: uuid::Uuid,\n    product_id: uuid::Uuid,\n) -> Result<Option<LineItem>, sqlx::Error>\nwhere\n"
        ));
    }
}
