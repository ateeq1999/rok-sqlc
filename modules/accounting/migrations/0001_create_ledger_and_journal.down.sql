drop trigger journal_lines_balanced on accounting.journal_lines;
drop trigger journal_entry_balanced on accounting.journal_entries;
drop function accounting.check_journal_entry_balanced();
drop table accounting.journal_lines;
drop table accounting.journal_entries;
drop table accounting.ledger_accounts;
drop schema accounting;
