ALTER TABLE device_metrics ADD COLUMN uptime_seconds INTEGER NOT NULL DEFAULT 0;
ALTER TABLE environment_metrics ADD COLUMN IAQ INTEGER NOT NULL DEFAULT 0;

ALTER TABLE nodes ADD COLUMN uptime_seconds INTEGER NULL;
ALTER TABLE nodes ADD COLUMN iaq INTEGER NULL;
