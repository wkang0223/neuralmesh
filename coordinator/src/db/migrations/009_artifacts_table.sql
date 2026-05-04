-- Migration 009: artifact blobs stored in Postgres so they survive redeployments.
-- Replaces filesystem storage at /var/neuralmesh/artifacts/.

CREATE TABLE IF NOT EXISTS artifacts (
    hash       TEXT        PRIMARY KEY,          -- hex MD5 of the bundle
    bundle_gz  BYTEA       NOT NULL,             -- raw tar.gz bytes
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_artifacts_created_at ON artifacts (created_at DESC);
