-- Migration 005: GPU hour tracking and milestone rewards

-- Track total GPU compute hours contributed per provider
ALTER TABLE providers
    ADD COLUMN IF NOT EXISTS total_gpu_hours_contributed DOUBLE PRECISION DEFAULT 0.0;

-- Track which milestone rewards have been issued (prevents double-paying)
CREATE TABLE IF NOT EXISTS milestone_rewards (
    id              BIGSERIAL PRIMARY KEY,
    provider_id     TEXT NOT NULL REFERENCES providers(provider_id),
    milestone_hours INTEGER NOT NULL,       -- e.g. 8, 16, 24, ...
    reward_hc       DOUBLE PRECISION NOT NULL DEFAULT 50.0,
    account_id      TEXT NOT NULL,
    tx_id           TEXT,                  -- ledger transaction ID
    created_at      TIMESTAMPTZ DEFAULT now(),
    UNIQUE(provider_id, milestone_hours)
);

CREATE INDEX IF NOT EXISTS idx_milestone_rewards_provider ON milestone_rewards(provider_id);
