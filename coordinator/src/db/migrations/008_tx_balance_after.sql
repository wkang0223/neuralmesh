-- Migration 008: balance_after column on transactions
-- Stores the account's available_nmc immediately after this transaction.
-- Used by hatch wallet history to show running balance per transaction.

ALTER TABLE transactions ADD COLUMN IF NOT EXISTS balance_after DOUBLE PRECISION;
