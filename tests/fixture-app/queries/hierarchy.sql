-- Generated into tests/fixture-app/src/db/queries/hierarchy.rs.

-- A partial unique index is an upsert target when the target repeats its
-- condition: `soft_notes_code_unique_index` covers live rows only.
-- name: UpsertLiveSoftNote :one
INSERT INTO soft_notes (code, body) VALUES ($1, $2)
ON CONFLICT (code) WHERE deleted_at IS NULL DO UPDATE SET body = EXCLUDED.body
RETURNING *;

-- A recursive CTE: the anchor types the columns, the recursive term walks
-- `parent_id` down the tree.
-- name: OrgSubtree :many
WITH RECURSIVE tree (id, name, depth) AS (
    SELECT id, name, 0::int FROM org_units WHERE id = $1
    UNION ALL
    SELECT o.id, o.name, t.depth + 1 FROM org_units o JOIN tree t ON o.parent_id = t.id
)
SELECT id, name, depth FROM tree ORDER BY depth, id;
