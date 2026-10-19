//! The helpers the generated checks and transforms of string rules call,
//! written once into `shared/validation.rs` when any feature's rules need
//! them, laid out as rustfmt lays them out. The ones a `dto.rs` calls are
//! public; the data tables some of them read are in
//! [`super::rule_tables`]. Each starts with a newline, the blank line
//! before it.

/// The `check` helper.
pub const CHECK: &str = r#"
/// `Ok` when `valid`, otherwise the error `code`.
pub fn check(valid: bool, code: &'static str) -> Result<(), validator::ValidationError> {
    if valid {
        Ok(())
    } else {
        Err(validator::ValidationError::new(code))
    }
}
"#;

/// The `matches_regex` helper.
pub const MATCHES_REGEX: &str = r#"
/// Whether `value` matches `regex`, compiled from a pattern rok-sqlc checked.
#[must_use]
pub fn matches_regex(value: &str, regex: Option<&regex::Regex>) -> bool {
    regex.is_some_and(|regex| regex.is_match(value))
}
"#;

/// The `letters` helper.
pub const LETTERS: &str = r#"
/// The `alpha` and `alphaNumeric` rules: one or more ASCII letters (and
/// digits, with `digits`), and whitespace, `-` or `_` where allowed.
#[must_use]
pub fn letters(value: &str, digits: bool, spaces: bool, dashes: bool, underscores: bool) -> bool {
    !value.is_empty()
        && value.chars().all(|c| {
            c.is_ascii_alphabetic()
                || (digits && c.is_ascii_digit())
                || (spaces && c.is_whitespace())
                || (dashes && c == '-')
                || (underscores && c == '_')
        })
}
"#;

/// The `is_uuid` helper.
pub const IS_UUID: &str = r#"
/// validator.js's `isUUID`: hex digits in the 8-4-4-4-12 shape, with the
/// RFC 9562 variant and one of `versions`; with none, any version from 1 to
/// 8, or the nil or max UUID.
#[must_use]
pub fn is_uuid(value: &str, versions: &[u8]) -> bool {
    let bytes = value.as_bytes();
    let shaped = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if !shaped {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if versions.is_empty()
        && (lower == "00000000-0000-0000-0000-000000000000"
            || lower == "ffffffff-ffff-ffff-ffff-ffffffffffff")
    {
        return true;
    }
    let (version, variant) = (bytes[14], bytes[19].to_ascii_lowercase());
    let known = if versions.is_empty() {
        (b'1'..=b'8').contains(&version)
    } else {
        versions.iter().any(|allowed| version == b'0' + allowed)
    };
    known && matches!(variant, b'8' | b'9' | b'a' | b'b')
}
"#;

/// The `is_ulid` helper.
pub const IS_ULID: &str = r#"
/// The `ulid` rule: 26 Crockford base32 characters, the first at most `7`.
#[must_use]
pub fn is_ulid(value: &str) -> bool {
    value.len() == 26
        && value.starts_with(|c: char| ('0'..='7').contains(&c))
        && value.chars().all(|c| {
            c.is_ascii_alphanumeric() && !matches!(c.to_ascii_uppercase(), 'I' | 'L' | 'O' | 'U')
        })
}
"#;

/// The `is_hex_code` helper.
pub const IS_HEX_CODE: &str = r#"
/// The `hexCode` rule: `#` and 3, 4, 6 or 8 hex digits.
#[must_use]
pub fn is_hex_code(value: &str) -> bool {
    value.strip_prefix('#').is_some_and(|hex| {
        matches!(hex.len(), 3 | 4 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit())
    })
}
"#;

/// The `is_jwt` helper.
pub const IS_JWT: &str = r#"
/// validator.js's `isJWT`: three parts joined by `.`, each URL-safe base64
/// without padding (or empty, as validator.js allows).
#[must_use]
pub fn is_jwt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            part.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
}
"#;

/// The `is_iban` helper.
pub const IS_IBAN: &str = r#"
/// validator.js's `isIBAN`: whitespace and `-` removed and the rest
/// uppercased, a country's format from [`IBAN_FORMATS`], and the mod-97
/// check.
#[must_use]
pub fn is_iban(value: &str) -> bool {
    let iban: String = value
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_uppercase();
    let Some((_, format)) = IBAN_FORMATS
        .iter()
        .find(|(country, _)| iban.starts_with(country))
    else {
        return false;
    };
    // The format after the country: counts of digits (`n`), letters (`a`)
    // or either (`c`).
    let mut chars = iban.chars().skip(2);
    let mut count = 0;
    for symbol in format.chars() {
        if let Some(digit) = symbol.to_digit(10) {
            count = count * 10 + digit;
            continue;
        }
        for _ in 0..count {
            let fits = chars.next().is_some_and(|c| match symbol {
                'n' => c.is_ascii_digit(),
                'a' => c.is_ascii_uppercase(),
                _ => c.is_ascii_uppercase() || c.is_ascii_digit(),
            });
            if !fits {
                return false;
            }
        }
        count = 0;
    }
    if chars.next().is_some() {
        return false;
    }
    // The first four characters moved to the end, letters as 10 to 35.
    let mut remainder = 0;
    for c in iban.chars().skip(4).chain(iban.chars().take(4)) {
        let Some(value) = c.to_digit(36) else {
            return false;
        };
        let scale = if value < 10 { 10 } else { 100 };
        remainder = (remainder * scale + value) % 97;
    }
    remainder == 1
}
"#;

/// The `is_coordinates` helper.
pub const IS_COORDINATES: &str = r#"
/// validator.js's `isLatLong`: `latitude,longitude` in decimal degrees,
/// optionally in parentheses. As in validator.js, text after a second comma
/// is not read.
#[must_use]
pub fn is_coordinates(value: &str) -> bool {
    let mut parts = value.split(',');
    let (Some(latitude), Some(longitude)) = (parts.next(), parts.next()) else {
        return false;
    };
    if latitude.starts_with('(') != longitude.ends_with(')') {
        return false;
    }
    let latitude = latitude.strip_prefix('(').unwrap_or(latitude);
    let longitude = longitude.strip_suffix(')').unwrap_or(longitude);
    let longitude = longitude
        .strip_prefix(char::is_whitespace)
        .unwrap_or(longitude);
    degrees(latitude, 90) && degrees(longitude, 180)
}
"#;

/// The `degrees` helper.
pub const DEGREES: &str = r#"
/// A signed number of degrees up to `max` (90 or 180), as validator.js's
/// `isLatLong` patterns write one.
fn degrees(text: &str, max: u32) -> bool {
    let text = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|fraction| !digits(fraction)) {
        return false;
    }
    let Ok(number) = whole.parse::<u32>() else {
        return false;
    };
    if number == max {
        return fraction.is_none_or(|fraction| fraction.bytes().all(|byte| byte == b'0'));
    }
    if max == 90 {
        whole.len() == 1 || (whole.len() == 2 && (10..90).contains(&number))
    } else {
        whole.len() <= 2 || (whole.len() == 3 && (100..180).contains(&number))
    }
}
"#;

/// The `is_credit_card` helper.
pub const IS_CREDIT_CARD: &str = r#"
/// validator.js's `isCreditCard`: spaces and dashes removed, a number of one
/// of `providers` (of any, when none), passing the Luhn check.
#[must_use]
pub fn is_credit_card(value: &str, providers: &[&str]) -> bool {
    let number: String = value.chars().filter(|c| *c != ' ' && *c != '-').collect();
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    let len = number.len();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|prefix| number.starts_with(prefix));
    let between = |at: usize, low: u32, high: u32| {
        number
            .get(at..at + low.to_string().len())
            .and_then(|digits| digits.parse::<u32>().ok())
            .is_some_and(|digits| (low..=high).contains(&digits))
    };
    let of = |provider: &str| match provider {
        "amex" => len == 15 && starts(&["34", "37"]),
        "dinersclub" => len == 14 && (between(0, 300, 305) || starts(&["36", "38"])),
        "discover" => (16..=19).contains(&len) && starts(&["6011", "65"]),
        "jcb" => (len == 15 && starts(&["2131", "1800"])) || (len == 16 && starts(&["35"])),
        // validator.js's pattern: a `51` to `55` prefix, or 16 digits from
        // `2221` to `2720` at the end.
        "mastercard" => {
            (len >= 4 && between(0, 51, 55)) || (len >= 16 && between(len - 16, 2221, 2720))
        }
        "unionpay" => {
            (len == 16 && starts(&["62", "67"])) || ((16..=19).contains(&len) && starts(&["81"]))
        }
        _ => (len == 13 || (16..=19).contains(&len)) && starts(&["4"]),
    };
    let known = if providers.is_empty() {
        CARD_PROVIDERS.iter().any(|provider| of(provider))
    } else {
        providers.iter().any(|provider| of(provider))
    };
    known && luhn(&number)
}
"#;

/// The `luhn` helper.
pub const LUHN: &str = r#"
/// The Luhn check of a number of ASCII digits.
fn luhn(number: &str) -> bool {
    let mut sum = 0;
    for (index, byte) in number.bytes().rev().enumerate() {
        let digit = u32::from(byte - b'0');
        sum += if index % 2 == 1 {
            let doubled = digit * 2;
            if doubled >= 10 { doubled - 9 } else { doubled }
        } else {
            digit
        };
    }
    sum % 10 == 0
}
"#;

/// The `normalize_email` helper.
pub const NORMALIZE_EMAIL: &str = r#"
/// validator.js's `normalizeEmail` with its defaults, which the
/// `normalizeEmail` transform uses: the domain lowercased, and the local part
/// lowercased; for Gmail, the local part without a `+` subaddress or single
/// dots, and `googlemail.com` made `gmail.com`; for iCloud and Outlook.com,
/// without a `+` subaddress; for Yahoo, without a `-` subaddress; for
/// Yandex, the domain made `yandex.ru`. A value without `@`, or whose local
/// part would be left empty, is kept as given.
#[must_use]
pub fn normalize_email(value: &str) -> String {
    let Some((user, domain)) = value.rsplit_once('@') else {
        return value.to_owned();
    };
    let domain = domain.to_lowercase();
    let plus = |user: &str| {
        user.split_once('+')
            .map_or(user, |(user, _)| user)
            .to_owned()
    };
    let (user, domain) = match domain.as_str() {
        "gmail.com" | "googlemail.com" => (remove_single_dots(&plus(user)), "gmail.com".to_owned()),
        name if ICLOUD.contains(&name) || OUTLOOK.contains(&name) => (plus(user), domain),
        name if YAHOO.contains(&name) => {
            let user = user.rsplit_once('-').map_or(user, |(user, _)| user);
            (user.to_owned(), domain)
        }
        name if YANDEX.contains(&name) => (user.to_owned(), "yandex.ru".to_owned()),
        _ => (user.to_owned(), domain),
    };
    if user.is_empty() {
        return value.to_owned();
    }
    format!("{}@{domain}", user.to_lowercase())
}
"#;

/// The `remove_single_dots` helper.
pub const REMOVE_SINGLE_DOTS: &str = r#"
/// `user` without its single dots; a run of two or more stays.
fn remove_single_dots(user: &str) -> String {
    let mut kept = String::new();
    let mut chars = user.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '.' {
            kept.push(c);
            continue;
        }
        let mut run = 1;
        while chars.next_if_eq(&'.').is_some() {
            run += 1;
        }
        if run > 1 {
            kept.push_str(&".".repeat(run));
        }
    }
    kept
}
"#;

/// The `transformed` helper.
pub const TRANSFORMED: &str = r#"
/// A body field a column's transforms apply to: text, optional text, or
/// nullable text, where `null` is `Some(None)`.
pub trait Transformed<'de>: Sized {
    /// Deserializes the field, transformed by `transform`.
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>;
}

impl<'de> Transformed<'de> for String {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer).map(|value: String| transform(&value))
    }
}

impl<'de> Transformed<'de> for Option<String> {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer)
            .map(|value: Option<String>| value.map(|value| transform(&value)))
    }
}

impl<'de> Transformed<'de> for Option<Option<String>> {
    fn transformed<D>(deserializer: D, transform: fn(&str) -> String) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::Deserialize::deserialize(deserializer)
            .map(|value: Option<String>| Some(value.map(|value| transform(&value))))
    }
}
"#;

/// The `is_url` helper, with the `url_port` and `is_domain` it calls.
pub const IS_URL: &str = r#"
/// The `url` rule: validator.js's `isURL` with its default options. An
/// optional `http`, `https` or `ftp` scheme, optional credentials, a host
/// that is an IP address or a domain with a top-level domain, an optional
/// port, then any path, query and fragment.
#[must_use]
pub fn is_url(value: &str) -> bool {
    let unsafe_char = |c: char| c.is_whitespace() || matches!(c, '<' | '>' | '\u{feff}');
    if value.is_empty()
        || value.encode_utf16().count() > 2084
        || value.starts_with("mailto:")
        || value.chars().any(unsafe_char)
    {
        return false;
    }
    let value = value.split(['#', '?']).next().unwrap_or_default();
    let rest = match value.split_once("://") {
        Some((scheme, rest)) => {
            let scheme = scheme.to_lowercase();
            if !matches!(scheme.as_str(), "http" | "https" | "ftp") {
                return false;
            }
            rest
        }
        None if value.starts_with("//") => return false,
        None => value,
    };
    let authority = rest.split('/').next().unwrap_or_default();
    let host = match authority.split_once('@') {
        Some((credentials, host)) => {
            let colons = credentials.matches(':').count();
            if credentials.is_empty() || credentials == ":" || colons > 1 {
                return false;
            }
            host
        }
        None => authority,
    };
    if let Some(bracketed) = host.strip_prefix('[') {
        let Some((address, after)) = bracketed.split_once(']') else {
            return false;
        };
        let port = match after.strip_prefix(':') {
            Some(port) if !port.is_empty() => port,
            None if after.is_empty() => "",
            _ => return false,
        };
        let (address, zone) = address.split_once('%').unwrap_or((address, "a"));
        let zone_valid = !zone.is_empty()
            && zone
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':'));
        return url_port(port) && zone_valid && address.parse::<std::net::Ipv6Addr>().is_ok();
    }
    let (name, port) = host.split_once(':').unwrap_or((host, ""));
    url_port(port) && (name.parse::<std::net::Ipv4Addr>().is_ok() || is_domain(name))
}

/// A URL's port: none, or a number from 1 to 65535.
fn url_port(port: &str) -> bool {
    port.is_empty()
        || (port.bytes().all(|b| b.is_ascii_digit())
            && port
                .parse::<u32>()
                .is_ok_and(|port| (1..=65535).contains(&port)))
}

/// validator.js's `isFQDN` as `isURL` calls it: two or more labels of
/// letters, digits and `-` (not first or last), at most 63 long, the last a
/// top-level domain of two or more letters or an `xn--` name.
fn is_domain(name: &str) -> bool {
    let labels: Vec<&str> = name.split('.').collect();
    let Some(tld) = labels.last().filter(|_| labels.len() > 1) else {
        return false;
    };
    // A letter of validator.js's pattern, `[a-z¡-¨ª-퟿
    // 豈-﷏ﷰ-￯]` ignoring case: no surrogate is a `char`.
    let letter = |c: char| {
        c.is_ascii_alphabetic()
            || (('\u{a1}'..='\u{ffef}').contains(&c)
                && !matches!(c, '\u{a9}' | '\u{e000}'..='\u{f8ff}' | '\u{fdd0}'..='\u{fdef}'))
    };
    let punycode = tld.len() >= 4
        && tld
            .get(..2)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("xn"))
        && tld.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let named = tld.chars().count() >= 2 && tld.chars().all(letter);
    (named || punycode)
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.encode_utf16().count() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| {
                    c.is_ascii_alphanumeric()
                        || c == '-'
                        || (c >= '\u{a1}' && !('\u{ff01}'..='\u{ff5e}').contains(&c))
                })
        })
}
"#;
