CREATE TYPE employment_type AS ENUM ('FULL_TIME', 'PART_TIME', 'CONTRACT');

CREATE TABLE departments (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE employees (
    id UUID PRIMARY KEY,
    employee_no TEXT NOT NULL,
    full_name TEXT NOT NULL,
    department_id BIGINT REFERENCES departments (id),
    employment_type EMPLOYMENT_TYPE NOT NULL,
    hired_on DATE NOT NULL,
    clock_out TIME,
    tenure INTERVAL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
