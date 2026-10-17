-- Generated into tests/fixture-app/src/validated_db/ with
-- `derive = ["serde", "validate"]` (ADR 0015): declared lengths and
-- single-column range checks become validation rules on the create inputs
-- and update patches, which tests/validate.rs exercises without a database.

CREATE TABLE teams (
    -- A text key: `TeamId` measures the text inside, so the length rule
    -- applies through the typed id, here and on `players.team_code`.
    code VARCHAR(8) PRIMARY KEY,
    name VARCHAR(40) NOT NULL CHECK (char_length(name) >= 2)
);

CREATE TABLE players (
    id BIGSERIAL PRIMARY KEY,
    team_code VARCHAR(8) REFERENCES teams (code),
    handle VARCHAR(20) NOT NULL,
    bio TEXT CHECK (length(bio) <= 200),
    jersey INTEGER NOT NULL CHECK (jersey BETWEEN 0 AND 99),
    rating DOUBLE PRECISION CHECK (rating > 0 AND rating <= 10),
    salary NUMERIC(12, 2) NOT NULL DEFAULT 0 CHECK (salary >= 0),
    nickname VARCHAR(12) DEFAULT 'rookie',
    -- Not a rule: only the database checks it.
    joined_year INTEGER CHECK (joined_year > 1900 OR joined_year IS NULL)
);

CREATE TABLE seasons (
    -- An integer key with a range: `SeasonId` compares the number inside,
    -- so the range rule applies through the typed id, here and on
    -- `previous_year`.
    year INTEGER PRIMARY KEY CHECK (year BETWEEN 1900 AND 2100),
    previous_year INTEGER REFERENCES seasons (year) CHECK (previous_year >= 1900),
    title VARCHAR(40) NOT NULL
);
