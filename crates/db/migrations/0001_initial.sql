CREATE TABLE applications (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    repository_url TEXT NOT NULL,
    container_port INTEGER NOT NULL CHECK (container_port BETWEEN 1 AND 65535),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE deployments (
    id UUID PRIMARY KEY,
    application_id UUID NOT NULL REFERENCES applications(id),
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN (
        'queued', 'fetching', 'building', 'starting', 'checking_health', 'ready', 'failed'
    )),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX deployments_application_history ON deployments(application_id, created_at DESC, id DESC);
CREATE INDEX deployments_pending ON deployments(created_at, id) WHERE status = 'queued';
