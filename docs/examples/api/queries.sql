-- SQL the data layer doesn't generate, called by ProjectsService.

-- name: OpenTask :execrows
UPDATE projects SET open_tasks = open_tasks + 1
WHERE id = $1 AND owner_id = $2;

-- name: ProjectStats :one
SELECT count(*) AS projects, coalesce(sum(open_tasks), 0)::BIGINT AS open_tasks
FROM projects
WHERE owner_id = $1;
