ALTER TABLE mesh_packets ADD COLUMN received_at INTEGER NOT NULL DEFAULT 0;
-- Best value to use is created at. From now on the received_at will be set to the creation timestamp of the service envelope.
UPDATE mesh_packets SET received_at = created_at;
