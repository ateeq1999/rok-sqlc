//! Layout helpers for the API feature renderers, laying code out as rustfmt
//! does where a name's length decides it.

use crate::generator::{MAX_WIDTH, use_list};

/// A function signature at `indent` ending with ` {`: `{head}(params) ->
/// {returns}` on one line when it fits, otherwise one parameter per line.
/// An empty `returns` has no arrow.
pub fn push_signature(
    output: &mut String,
    indent: usize,
    head: &str,
    params: &[String],
    returns: &str,
) {
    let pad = " ".repeat(indent);
    let arrow = if returns.is_empty() {
        String::new()
    } else {
        format!(" -> {returns}")
    };
    let one_line = format!("{pad}{head}({}){arrow} {{", params.join(", "));
    if one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("{pad}{head}(\n"));
    for param in params {
        output.push_str(&format!("{pad}    {param},\n"));
    }
    output.push_str(&format!("{pad}){arrow} {{\n"));
}

/// rustfmt's default `struct_lit_width`: a struct literal whose fields take
/// more than this goes one field per line.
const STRUCT_LIT_WIDTH: usize = 18;

/// rustfmt's default `attr_fn_like_width`: an attribute whose arguments
/// take more than this goes one argument per line.
const ATTR_WIDTH: usize = 70;

/// The struct literal `{name} { fields }` at `indent`, on its own lines:
/// on one line when the fields fit `struct_lit_width`, otherwise one field
/// per line.
pub fn push_struct_lit(output: &mut String, indent: usize, name: &str, fields: &[String]) {
    let pad = " ".repeat(indent);
    let joined = fields.join(", ");
    let one_line = format!("{pad}{name} {{ {joined} }}");
    if joined.len() <= STRUCT_LIT_WIDTH && one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    output.push_str(&format!("{pad}{name} {{\n"));
    for field in fields {
        let line = format!("{pad}    {field},");
        // rustfmt breaks a too-long `name: binding.name` before the `.`.
        let split = field.split_once(": ").and_then(|(name, value)| {
            value
                .split_once('.')
                .map(|(binding, rest)| (name, binding, rest))
        });
        match split {
            Some((name, binding, rest)) if line.len() > MAX_WIDTH => {
                output.push_str(&format!(
                    "{pad}    {name}: {binding}\n{pad}        .{rest},\n"
                ));
            }
            _ => {
                output.push_str(&line);
                output.push('\n');
            }
        }
    }
    output.push_str(&format!("{pad}}}\n"));
}

/// The field attribute `#[{name}(args)]`: on one line when the arguments
/// fit `attr_fn_like_width`, otherwise one per line.
pub fn push_field_attr(output: &mut String, name: &str, args: &[String]) {
    let joined = args.join(", ");
    let one_line = format!("    #[{name}({joined})]");
    if joined.len() <= ATTR_WIDTH && one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    // rustfmt leaves no comma after an attribute's last argument.
    output.push_str(&format!(
        "    #[{name}(\n        {}\n    )]\n",
        args.join(",\n        ")
    ));
}

/// The `use` lines of a file, collected while its items are rendered:
/// external crates first, then a blank line and the crate's own paths,
/// each group in rustfmt's order, one line per module.
#[derive(Debug, Default)]
pub struct Uses {
    /// The items imported, by the module they come from.
    modules: std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl Uses {
    /// Imports `path`'s last segment from the rest, such as
    /// `crate::error::AppError`.
    pub fn add(&mut self, path: &str) {
        if let Some((module, item)) = path.rsplit_once("::") {
            self.modules
                .entry(module.to_owned())
                .or_default()
                .insert(item.to_owned());
        }
    }

    /// The `use` lines, ending with a blank line; nothing without any.
    pub fn render(&self) -> String {
        let local = |module: &str| {
            let first = module.split("::").next().unwrap_or_default();
            matches!(first, "super" | "crate" | "self")
        };
        let mut output = String::new();
        for group in [false, true] {
            let mut lines: Vec<String> = self
                .modules
                .iter()
                .filter(|(module, _)| local(module) == group)
                .map(|(module, items)| {
                    let items: Vec<String> = items.iter().cloned().collect();
                    use_list(module, &items)
                })
                .collect();
            lines.sort_by_key(|line| {
                line.trim_start_matches("use ")
                    .trim_end_matches(";\n")
                    .split("::")
                    .map(|segment| (segment != "super", segment.to_owned()))
                    .collect::<Vec<_>>()
            });
            if !lines.is_empty() {
                output.push_str(&lines.concat());
                output.push('\n');
            }
        }
        output
    }
}

/// `{lead}{callee}({args}){tail}` at `indent`, laid out as rustfmt lays out
/// a call: on one line when it fits (its arguments within
/// `fn_call_width`); for a `let`, with the call moved below the `=`; else
/// one argument per line. A `tail` starting with `.` (`.await?;`) goes on
/// its own line after a broken call.
pub fn push_call(
    output: &mut String,
    indent: usize,
    lead: &str,
    callee: &str,
    args: &[String],
    tail: &str,
) {
    if let Some((receiver, method)) = callee.split_once('.') {
        push_method_call(output, indent, lead, receiver, method, args, tail);
        return;
    }
    let pad = " ".repeat(indent);
    let joined = args.join(", ");
    let call = format!("{callee}({joined}){tail}");
    let fits_args = joined.len() <= crate::generator::FN_CALL_WIDTH;
    // A chained `.await?;` leaves one column less (found by rustfmt).
    let width = if tail.starts_with('.') {
        MAX_WIDTH - 1
    } else {
        MAX_WIDTH
    };
    if fits_args && pad.len() + lead.len() + call.len() <= width {
        output.push_str(&format!("{pad}{lead}{call}\n"));
        return;
    }
    let inner = " ".repeat(indent + 4);
    if fits_args && lead.starts_with("let ") {
        if inner.len() + call.len() <= width {
            output.push_str(&format!("{pad}{}\n{inner}{call}\n", lead.trim_end()));
            return;
        }
        // The call alone fits below the `=`, its `.await?` below it.
        let bare = format!("{callee}({joined})");
        if tail.starts_with('.') && inner.len() + bare.len() <= MAX_WIDTH {
            output.push_str(&format!(
                "{pad}{}\n{inner}{bare}\n{inner}    {tail}\n",
                lead.trim_end()
            ));
            return;
        }
    }
    output.push_str(&format!("{pad}{lead}{callee}(\n"));
    // rustfmt lays simple arguments of at most 10 columns
    // (`short_array_element_width_threshold`) side by side.
    let short = args.iter().all(|arg| {
        arg.len() <= 10
            && arg
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '&' | '*' | ' '))
    });
    if fits_args && short {
        output.push_str(&format!("{inner}{joined},\n"));
    } else {
        for arg in args {
            output.push_str(&format!("{inner}{arg},\n"));
        }
    }
    if tail.starts_with('.') {
        output.push_str(&format!("{pad})\n{pad}{tail}\n"));
    } else {
        output.push_str(&format!("{pad}){tail}\n"));
    }
}

/// rustfmt's default `chain_width`: a chain longer than this goes one call
/// per line.
const CHAIN_WIDTH: usize = 60;

/// `{lead}{receiver}.{method}({args}){tail}` at `indent`, a method call
/// chained with `tail` (`.await?;`), laid out as rustfmt lays out a chain:
/// on one line within `chain_width`; else one call per line below the
/// receiver, except that a receiver no wider than an indent (`self`)
/// opening a statement keeps the first call.
fn push_method_call(
    output: &mut String,
    indent: usize,
    lead: &str,
    receiver: &str,
    method: &str,
    args: &[String],
    tail: &str,
) {
    let pad = " ".repeat(indent);
    let joined = args.join(", ");
    let fits_args = joined.len() <= crate::generator::FN_CALL_WIDTH;
    let end = tail.trim_end_matches(';');
    let chain = format!("{receiver}.{method}({joined}){end}");
    let one_line = format!("{pad}{lead}{chain}{}", &tail[end.len()..]);
    if fits_args && chain.len() < CHAIN_WIDTH && one_line.len() <= MAX_WIDTH {
        output.push_str(&one_line);
        output.push('\n');
        return;
    }
    let call = format!("{method}({joined})");
    if lead.is_empty() && receiver.len() <= 4 {
        let first = format!("{pad}{receiver}.{call}");
        if fits_args && first.len() <= MAX_WIDTH {
            output.push_str(&format!("{first}\n{pad}    {tail}\n"));
        } else {
            output.push_str(&format!("{pad}{receiver}.{method}(\n"));
            for arg in args {
                output.push_str(&format!("{pad}    {arg},\n"));
            }
            output.push_str(&format!("{pad})\n{pad}{tail}\n"));
        }
        return;
    }
    output.push_str(&format!("{pad}{lead}{receiver}\n"));
    let inner = " ".repeat(indent + 4);
    if fits_args && inner.len() + 1 + call.len() <= MAX_WIDTH {
        output.push_str(&format!("{inner}.{call}\n"));
    } else {
        output.push_str(&format!("{inner}.{method}(\n"));
        for arg in args {
            output.push_str(&format!("{inner}    {arg},\n"));
        }
        output.push_str(&format!("{inner})\n"));
    }
    output.push_str(&format!("{inner}{tail}\n"));
}

/// `impl {item} for {ty} {` and its line break, the clauses on lines of
/// their own when the header is too long, as rustfmt breaks it.
pub fn impl_head(item: &str, ty: &str) -> String {
    let one_line = format!("impl {item} for {ty} {{\n");
    if one_line.len() - 1 <= MAX_WIDTH {
        one_line
    } else {
        format!("impl {item}\n    for {ty}\n{{\n")
    }
}
