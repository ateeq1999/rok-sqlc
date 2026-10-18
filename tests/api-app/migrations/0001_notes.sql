-- The fixture application's schema. Ticket 06's API features extend it.
CREATE TABLE notes (
    id UUID PRIMARY KEY,
    title TEXT NOT NULL
);
