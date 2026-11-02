ALTER TABLE applications
    ADD COLUMN active_deployment_id UUID REFERENCES deployments(id),
    ADD COLUMN deleted_at TIMESTAMPTZ,
    ADD COLUMN runtime_state TEXT NOT NULL DEFAULT 'unknown',
    ADD COLUMN checked_at TIMESTAMPTZ;
ALTER TABLE applications DROP CONSTRAINT applications_name_key;
CREATE UNIQUE INDEX applications_active_name ON applications(name) WHERE deleted_at IS NULL;
ALTER TABLE deployments
    ADD COLUMN operation TEXT NOT NULL DEFAULT 'deploy' CHECK (operation IN ('deploy','rollback','stop','delete','cleanup')),
    ADD COLUMN source_deployment_id UUID REFERENCES deployments(id),
    ADD COLUMN image_available BOOLEAN NOT NULL DEFAULT TRUE;
UPDATE applications a SET active_deployment_id = (
    SELECT d.id FROM deployments d WHERE d.application_id = a.id AND d.status = 'ready'
    ORDER BY d.finished_at DESC NULLS LAST, d.created_at DESC LIMIT 1
);
CREATE TABLE worker_heartbeat (id INTEGER PRIMARY KEY CHECK (id = 1), seen_at TIMESTAMPTZ NOT NULL);
