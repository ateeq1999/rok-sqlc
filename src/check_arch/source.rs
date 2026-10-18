//! What one Rust file refers to, read with `syn`: every path it names (in
//! `use` items, expressions, types, attributes, derives and macro
//! arguments), and every call rule 6 forbids. Comments and strings are not
//! code, so they never count; neither do `#[cfg(test)]` and `#[test]` items.

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{Attribute, Expr, Path, Token, UseTree};

/// A path as written in the source.
#[derive(Debug, Clone)]
pub(super) struct Reference {
    /// Whether it starts with `::`.
    pub(super) leading_colon: bool,
    /// Its segments' identifiers, generics left out; a glob ends with `*`.
    pub(super) segments: Vec<String>,
    /// The 1-based line it starts on.
    pub(super) line: usize,
    /// The name a `use` leaf binds (`None` for a glob, `_`, or a path that
    /// is not in a `use`).
    pub(super) binds: Option<String>,
}

/// A forbidden call or macro (rule 6).
#[derive(Debug)]
pub(super) struct Forbidden {
    /// `unwrap()`, `panic!`, ...
    pub(super) what: String,
    pub(super) line: usize,
}

/// Everything rules 1, 2, 4, 5 and 6 need from one file.
#[derive(Debug, Default)]
pub(super) struct Source {
    pub(super) references: Vec<Reference>,
    pub(super) forbidden: Vec<Forbidden>,
}

/// The methods and macros rule 6 forbids outside tests.
const FORBIDDEN_METHODS: &[&str] = &["unwrap", "expect"];
const FORBIDDEN_MACROS: &[&str] = &["panic", "todo", "println", "dbg"];

/// Reads `file`. A file that is test code as a whole (`#![cfg(test)]`)
/// refers to nothing.
pub(super) fn read(file: &syn::File) -> Source {
    let mut collector = Collector::default();
    if !is_test(&file.attrs) {
        collector.visit_file(file);
    }
    collector.source
}

fn line(span: Span) -> usize {
    span.start().line
}

/// Whether `attrs` mark test code: `#[cfg(test)]`, or a test attribute
/// (`#[test]`, `#[tokio::test]`, `#[sqlx::test]`, ...).
fn is_test(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        let path = attr.path();
        if path.is_ident("cfg") {
            return attr
                .parse_args::<syn::Ident>()
                .is_ok_and(|ident| ident == "test");
        }
        path.segments
            .last()
            .is_some_and(|last| last.ident == "test")
    })
}

#[derive(Default)]
struct Collector {
    source: Source,
}

impl Collector {
    fn path(&mut self, path: &Path, binds: Option<String>) {
        let Some(first) = path.segments.first() else {
            return;
        };
        self.source.references.push(Reference {
            leading_colon: path.leading_colon.is_some(),
            segments: path
                .segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect(),
            line: line(first.ident.span()),
            binds,
        });
    }

    /// Records each leaf of a `use` tree as its full path.
    fn use_tree(&mut self, leading_colon: bool, prefix: &mut Vec<String>, tree: &UseTree) {
        let leaf = |segments: Vec<String>, span: Span, binds: Option<String>| Reference {
            leading_colon,
            segments,
            line: line(span),
            binds,
        };
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.use_tree(leading_colon, prefix, &path.tree);
                prefix.pop();
            }
            UseTree::Name(name) => {
                let mut segments = prefix.clone();
                // `use a::{self}` names `a` itself.
                let binds = if name.ident == "self" {
                    segments.last().cloned()
                } else {
                    segments.push(name.ident.to_string());
                    Some(name.ident.to_string())
                };
                self.source
                    .references
                    .push(leaf(segments, name.ident.span(), binds));
            }
            UseTree::Rename(rename) => {
                let mut segments = prefix.clone();
                if rename.ident != "self" {
                    segments.push(rename.ident.to_string());
                }
                let binds = (rename.rename != "_").then(|| rename.rename.to_string());
                self.source
                    .references
                    .push(leaf(segments, rename.ident.span(), binds));
            }
            UseTree::Glob(glob) => {
                let mut segments = prefix.clone();
                segments.push("*".to_owned());
                self.source
                    .references
                    .push(leaf(segments, glob.star_token.span, None));
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.use_tree(leading_colon, prefix, tree);
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Collector {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attrs = match item {
            syn::Item::Const(item) => &item.attrs,
            syn::Item::Enum(item) => &item.attrs,
            syn::Item::ExternCrate(item) => &item.attrs,
            syn::Item::Fn(item) => &item.attrs,
            syn::Item::ForeignMod(item) => &item.attrs,
            syn::Item::Impl(item) => &item.attrs,
            syn::Item::Macro(item) => &item.attrs,
            syn::Item::Mod(item) => &item.attrs,
            syn::Item::Static(item) => &item.attrs,
            syn::Item::Struct(item) => &item.attrs,
            syn::Item::Trait(item) => &item.attrs,
            syn::Item::TraitAlias(item) => &item.attrs,
            syn::Item::Type(item) => &item.attrs,
            syn::Item::Union(item) => &item.attrs,
            syn::Item::Use(item) => &item.attrs,
            _ => return visit::visit_item(self, item),
        };
        if !is_test(attrs) {
            visit::visit_item(self, item);
        }
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if !is_test(&item.attrs) {
            visit::visit_impl_item_fn(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for attr in &item.attrs {
            self.visit_attribute(attr);
        }
        self.use_tree(item.leading_colon.is_some(), &mut Vec::new(), &item.tree);
    }

    fn visit_item_extern_crate(&mut self, item: &'ast syn::ItemExternCrate) {
        self.source.references.push(Reference {
            leading_colon: true,
            segments: vec![item.ident.to_string()],
            line: line(item.ident.span()),
            binds: None,
        });
    }

    fn visit_path(&mut self, path: &'ast Path) {
        self.path(path, None);
        visit::visit_path(self, path);
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        // A one-segment attribute is built in or a derive's helper
        // (`#[serde(...)]`, `#[sqlx(...)]`), not a path into a crate.
        if attr.path().segments.len() > 1 {
            self.path(attr.path(), None);
        }
        if attr.path().is_ident("derive") {
            let derives = attr.parse_args_with(Punctuated::<Path, Token![,]>::parse_terminated);
            for path in derives.iter().flatten() {
                self.path(path, None);
            }
        }
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if let Some(last) = mac.path.segments.last() {
            let name = last.ident.to_string();
            if FORBIDDEN_MACROS.contains(&name.as_str()) {
                self.source.forbidden.push(Forbidden {
                    what: format!("{name}!"),
                    line: line(last.ident.span()),
                });
            }
        }
        visit::visit_macro(self, mac);
        // Arguments that parse as expressions (`format!`, `vec!`, ...) are
        // code too.
        if let Ok(args) = mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
            for arg in &args {
                self.visit_expr(arg);
            }
        }
    }

    /// `Option::unwrap(x)`, `Result::expect(r, "...")` or an imported
    /// `unwrap(x)`: a call through a path whose last segment is a forbidden
    /// method. A path that is not called (`map(Option::unwrap)`) is not.
    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        if let Expr::Path(callee) = &*call.func
            && let Some(last) = callee.path.segments.last()
        {
            let name = last.ident.to_string();
            if FORBIDDEN_METHODS.contains(&name.as_str()) {
                self.source.forbidden.push(Forbidden {
                    what: format!("{name}()"),
                    line: line(last.ident.span()),
                });
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let name = call.method.to_string();
        if FORBIDDEN_METHODS.contains(&name.as_str()) {
            self.source.forbidden.push(Forbidden {
                what: format!("{name}()"),
                line: line(call.method.span()),
            });
        }
        visit::visit_expr_method_call(self, call);
    }
}
