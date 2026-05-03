-- Migration 007: script_name column on jobs
-- Stores the entry-point filename the CLI packed (e.g. "inference.py").
-- The agent uses this to pick the right file from the extracted bundle
-- instead of heuristic scanning.

ALTER TABLE jobs ADD COLUMN IF NOT EXISTS script_name TEXT;
