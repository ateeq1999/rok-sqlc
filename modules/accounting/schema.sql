CREATE TABLE accounts (
    id UUID PRIMARY KEY,
    code TEXT NOT NULL,
    name TEXT NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE journal_entries (
    id UUID PRIMARY KEY,
    entry_no BIGINT NOT NULL,
    memo TEXT,
    posted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    total_debit NUMERIC(12, 2) NOT NULL DEFAULT 0,
    total_credit NUMERIC(12, 2) NOT NULL DEFAULT 0
);

CREATE TABLE ledger_lines (
    id BIGSERIAL PRIMARY KEY,
    entry_id UUID NOT NULL REFERENCES journal_entries (id),
    account_id UUID NOT NULL REFERENCES accounts (id),
    amount NUMERIC(14, 2) NOT NULL,
    memo TEXT
);
