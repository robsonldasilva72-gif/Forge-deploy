ALTER TABLE deployments
    ADD COLUMN repository_url TEXT,
    ADD COLUMN container_port INTEGER,
    ADD COLUMN commit_sha TEXT,
    ADD COLUMN image_tag TEXT,
    ADD COLUMN container_id TEXT,
    ADD COLUMN host_port INTEGER CHECK (host_port BETWEEN 1 AND 65535),
    ADD COLUMN started_at TIMESTAMPTZ,
    ADD COLUMN finished_at TIMESTAMPTZ,
    ADD COLUMN error TEXT,
    ADD COLUMN logs TEXT NOT NULL DEFAULT '';

UPDATE deployments d SET repository_url = a.repository_url, container_port = a.container_port
FROM applications a WHERE d.application_id = a.id;

ALTER TABLE deployments
    ALTER COLUMN repository_url SET NOT NULL,
    ALTER COLUMN container_port SET NOT NULL,
    ADD CONSTRAINT deployment_port_valid CHECK (container_port BETWEEN 1 AND 65535);
