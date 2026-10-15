//! Mapping SQL identifiers to Rust identifiers.
//!
//! SQL names are never rewritten: generated SQL always uses the original
//! identifier, and `#[sqlx(rename)]` bridges any difference on the Rust side.

use heck::{ToSnakeCase, ToUpperCamelCase};

/// Derives a snake_case Rust identifier (field or module name) from a SQL
/// identifier, suffixing `_` when the result is a Rust keyword and prefixing
/// `_` when it would start with a digit.
pub fn rust_identifier(sql_name: &str) -> Result<String, String> {
    let snake = sql_name.to_snake_case();
    if snake.is_empty() || snake == "_" {
        return Err(format!(
            "`{sql_name}` does not contain any characters usable in a Rust identifier"
        ));
    }
    if snake.starts_with(|character: char| character.is_ascii_digit()) {
        return Ok(format!("_{snake}"));
    }
    if is_rust_keyword(&snake) {
        return Ok(format!("{snake}_"));
    }
    Ok(snake)
}

/// Derives the row struct name for a table: UpperCamelCase with the head noun
/// singularized. The head noun is the last word (`stock_levels` →
/// `StockLevel`), or the word before `_of_` (`units_of_measure` →
/// `UnitOfMeasure`).
pub fn struct_name(module_name: &str) -> String {
    let trimmed = module_name.trim_matches('_');
    let (noun_phrase, qualifier) = match trimmed.split_once("_of_") {
        Some((noun_phrase, rest)) => (noun_phrase, format!("_of_{rest}")),
        None => (trimmed, String::new()),
    };
    let (head, noun) = match noun_phrase.rsplit_once('_') {
        Some((head, noun)) => (format!("{head}_"), noun),
        None => (String::new(), noun_phrase),
    };
    let name = format!("{head}{}{qualifier}", singularize(noun));
    let camel = name.to_upper_camel_case();
    if camel.starts_with(|character: char| character.is_ascii_digit()) {
        format!("T{camel}")
    } else {
        camel
    }
}

/// Derives a type name (an enum type's Rust name) from a SQL name:
/// UpperCamelCase, prefixed with `T` when it would start with a digit.
/// Empty when the name has no usable characters.
pub fn type_name(sql_name: &str) -> String {
    let camel = sql_name.to_upper_camel_case();
    if camel.starts_with(|character: char| character.is_ascii_digit()) {
        format!("T{camel}")
    } else {
        camel
    }
}

/// Derives an enum variant name from a database label: UpperCamelCase
/// (`Very High` → `VeryHigh`), prefixed with `V` when it would start with a
/// digit, and suffixed with `_` for the keyword `Self`. `None` when the label
/// has no usable characters.
pub fn variant_name(label: &str) -> Option<String> {
    let camel = label.to_upper_camel_case();
    if camel.is_empty() {
        None
    } else if camel.starts_with(|character: char| character.is_ascii_digit()) {
        Some(format!("V{camel}"))
    } else if camel == "Self" {
        Some("Self_".to_owned())
    } else {
        Some(camel)
    }
}

/// Singularizes one lowercase English word using a small rule set plus the
/// irregular forms that commonly appear in table names. Unknown shapes are
/// returned unchanged rather than guessed at.
pub fn singularize(word: &str) -> String {
    if let Some((_, singular)) = IRREGULAR.iter().find(|(plural, _)| *plural == word) {
        return (*singular).to_owned();
    }
    if UNCOUNTABLE.contains(&word) || word.len() < 3 {
        return word.to_owned();
    }
    if let Some(stem) = word.strip_suffix("ies") {
        return format!("{stem}y");
    }
    for suffix in ["sses", "xes", "ches", "shes", "zzes"] {
        if word.ends_with(suffix) {
            return word[..word.len() - 2].to_owned();
        }
    }
    if word.ends_with("ss") || word.ends_with("us") || word.ends_with("is") {
        return word.to_owned();
    }
    match word.strip_suffix('s') {
        Some(stem) => stem.to_owned(),
        None => word.to_owned(),
    }
}

const IRREGULAR: &[(&str, &str)] = &[
    ("people", "person"),
    ("children", "child"),
    ("men", "man"),
    ("women", "woman"),
    ("mice", "mouse"),
    ("geese", "goose"),
    ("feet", "foot"),
    ("teeth", "tooth"),
    ("criteria", "criterion"),
    ("phenomena", "phenomenon"),
    ("indices", "index"),
    ("matrices", "matrix"),
    ("vertices", "vertex"),
    ("analyses", "analysis"),
    ("crises", "crisis"),
    ("theses", "thesis"),
    ("statuses", "status"),
    ("aliases", "alias"),
    ("buses", "bus"),
    ("quizzes", "quiz"),
    ("caches", "cache"),
    ("niches", "niche"),
    ("movies", "movie"),
    ("cookies", "cookie"),
    ("pies", "pie"),
    ("ties", "tie"),
    ("lies", "lie"),
];

const UNCOUNTABLE: &[&str] = &[
    "data",
    "media",
    "news",
    "series",
    "species",
    "metadata",
    "equipment",
    "information",
];

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singularizes_regular_and_irregular_plurals() {
        for (plural, singular) in [
            ("products", "product"),
            ("categories", "category"),
            ("addresses", "address"),
            ("boxes", "box"),
            ("batches", "batch"),
            ("carcasses", "carcass"),
            ("messages", "message"),
            ("balances", "balance"),
            ("warehouses", "warehouse"),
            ("statuses", "status"),
            ("people", "person"),
            ("caches", "cache"),
            ("status", "status"),
            ("access", "access"),
            ("analysis", "analysis"),
            ("data", "data"),
            ("series", "series"),
            ("measure", "measure"),
        ] {
            assert_eq!(singularize(plural), singular, "singular of {plural}");
        }
    }

    #[test]
    fn struct_names_singularize_only_the_last_word() {
        assert_eq!(struct_name("products"), "Product");
        assert_eq!(struct_name("stock_levels"), "StockLevel");
        assert_eq!(struct_name("user_profiles"), "UserProfile");
        assert_eq!(struct_name("user_branch_access"), "UserBranchAccess");
        assert_eq!(struct_name("change_log"), "ChangeLog");
        assert_eq!(struct_name("units_of_measure"), "UnitOfMeasure");
        assert_eq!(struct_name("bills_of_materials"), "BillOfMaterials");
        assert_eq!(struct_name("type_"), "Type");
        assert_eq!(struct_name("_2024_sales"), "T2024Sale");
    }

    #[test]
    fn rust_identifiers_escape_keywords_and_leading_digits() {
        assert_eq!(rust_identifier("type").unwrap(), "type_");
        assert_eq!(rust_identifier("Mixed Case").unwrap(), "mixed_case");
        assert_eq!(rust_identifier("1st_place").unwrap(), "_1st_place");
        assert_eq!(rust_identifier("select").unwrap(), "select");
        assert!(rust_identifier("%").is_err());
        assert!(rust_identifier("_").is_err());
    }
}
