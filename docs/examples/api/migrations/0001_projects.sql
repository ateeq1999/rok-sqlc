-- Projects and their tasks. Each table becomes an API feature under
-- src/features/, owned by the caller through `owner_id` (rok-sqlc.toml).

CREATE TABLE projects (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL,
    name        VARCHAR(80) NOT NULL,
    open_tasks  INTEGER NOT NULL DEFAULT 0 CHECK (open_tasks >= 0),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE tasks (
    id          UUID PRIMARY KEY,
    owner_id    UUID NOT NULL,
    project_id  UUID NOT NULL REFERENCES projects (id),
    title       VARCHAR(200) NOT NULL,
    done        BOOLEAN NOT NULL DEFAULT false,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
