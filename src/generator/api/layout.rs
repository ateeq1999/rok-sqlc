//! Layout helpers for the API feature renderers, laying code out as rustfmt
//! does where a name's length decides it.

use crate::generator::{MAX_WIDTH, use_list};

/// `use {prefix}::{items};`, nothing for no items, wrapped as rustfmt
/// wraps a long list. `items` are written in the order given.
pub fn push_use(output: &mut String, prefix: &str, items: &[&str]) {
    if items.is_empty() {
        return;
    }
    let items: Vec<String> = items.iter().map(|item| (*item).to_owned()).collect();
    output.push_str(&use_list(prefix, &items));
}

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
