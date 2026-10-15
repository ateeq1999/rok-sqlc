-- name: CreateDepartment :one
INSERT INTO departments (name)
VALUES ($1)
RETURNING id, name;

-- name: ListEmployeesByDepartment :many
SELECT id, employee_no, full_name, department_id, hired_on, created_at
FROM employees
WHERE department_id = $1
ORDER BY hired_on DESC
LIMIT $2 OFFSET $3;
