-- String rules (`[api.rules]`, 0.5): a text column for each rule and
-- transform. `profiles` is public, and `tests/fixture_compile/` configures
-- its rules.
CREATE TABLE profiles (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    shout TEXT,
    contact TEXT,
    website TEXT,
    ref_uuid TEXT,
    ref_ulid TEXT,
    ip TEXT,
    phone TEXT,
    first_name TEXT,
    handle TEXT,
    plain TEXT,
    slug TEXT,
    country TEXT,
    username TEXT,
    sku TEXT,
    file_name TEXT,
    bio TEXT,
    pin TEXT,
    color TEXT,
    iban TEXT,
    token TEXT,
    zip TEXT,
    card TEXT,
    location TEXT
);
