CREATE TABLE members (
    id BIGSERIAL PRIMARY KEY,
    name VARCHAR(80) NOT NULL CHECK (char_length(name) >= 1),
    handle VARCHAR(30) NOT NULL UNIQUE,
    age INTEGER CHECK (age BETWEEN 13 AND 130),
    balance NUMERIC(12, 2) NOT NULL DEFAULT 0 CHECK (balance >= 0),
    bio TEXT
);
