-- Migration 004: DMTCP job checkpoints + heartbeat-based job migration

-- Checkpoint snapshots — one row per saved checkpoint iteration
CREATE TABLE IF NOT EXISTS job_checkpoints (
    checkpoint_id   TEXT PRIMARY KEY,
    job_id          TEXT REFERENCES jobs(job_id) ON DELETE CASCADE,
    provider_id     TEXT,
    iteration       INTEGER NOT NULL DEFAULT 1,
    checkpoint_dir  TEXT,          -- local path on provider (phase 1)
    checkpoint_url  TEXT,          -- remote URL if uploaded to object store (phase 2)
    dmtcp_files     TEXT[],        -- list of .dmtcp file names
    elapsed_secs    BIGINT,
    created_at      TIMESTAMPTZ DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_checkpoints_job ON job_checkpoints(job_id);
CREATE INDEX IF NOT EXISTS idx_checkpoints_created ON job_checkpoints(created_at DESC);

-- Extend jobs table with checkpoint and heartbeat tracking
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS checkpoint_url     TEXT;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS checkpoint_iter    INTEGER DEFAULT 0;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS last_heartbeat     TIMESTAMPTZ;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS restore_attempts   INTEGER DEFAULT 0;
ALTER TABLE jobs ADD COLUMN IF NOT EXISTS failure_reason     TEXT;

-- Add 'migrating' state to job lifecycle comment (state column already TEXT)
-- Valid states: queued | matching | assigned | running | migrating | complete | failed | cancelled

-- Index for heartbeat watcher (finds stale running jobs)
CREATE INDEX IF NOT EXISTS idx_jobs_running_heartbeat
    ON jobs(state, last_heartbeat)
    WHERE state IN ('running', 'assigned');

-- Index for re-queueing checkpointed jobs
CREATE INDEX IF NOT EXISTS idx_jobs_migrating
    ON jobs(state)
    WHERE state = 'migrating';
