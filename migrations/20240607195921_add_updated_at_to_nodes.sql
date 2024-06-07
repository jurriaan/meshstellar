ALTER TABLE nodes ADD COLUMN updated_at INTEGER DEFAULT 0 NOT NULL;
UPDATE nodes SET updated_at = last_rx_time WHERE updated_at = 0;
UPDATE nodes SET updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000000000 WHERE updated_at > CAST(strftime('%s', 'now') AS INTEGER) * 1000000000;
