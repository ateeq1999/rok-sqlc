-- Placeholder for the actix template's schema (ticket 02 derives it).
CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL UNIQUE
);
