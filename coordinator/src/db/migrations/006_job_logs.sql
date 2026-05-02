-- Migration 006: job log streaming
-- Agents POST log chunks here; coordinator stores accumulated output.
-- GET /api/v1/jobs/:id/logs reads from this column with offset slicing.

ALTER TABLE jobs ADD COLUMN IF NOT EXISTS output_log TEXT;
