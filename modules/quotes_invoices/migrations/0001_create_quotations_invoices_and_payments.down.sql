drop table quotes_invoices.invoice_payments;
drop table quotes_invoices.invoice_lines;
alter table quotes_invoices.quotations drop constraint quotations_converted_invoice_foreign_key;
drop table quotes_invoices.invoices;
drop table quotes_invoices.quotation_lines;
drop table quotes_invoices.quotations;
drop schema quotes_invoices;
