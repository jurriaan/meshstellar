-- Add migration script here
CREATE TABLE "power_metrics" (
    "id" integer NOT NULL PRIMARY KEY AUTOINCREMENT,
    "mesh_packet_id" integer UNIQUE NOT NULL,
    "node_id" integer NOT NULL,
    "time" integer,
    "ch1_voltage" real,
    "ch1_current" real,
    "ch2_voltage" real,
    "ch2_current" real,
    "ch3_voltage" real,
    "ch3_current" real,
    FOREIGN KEY ("mesh_packet_id") REFERENCES "mesh_packets" ("id") ON DELETE CASCADE
) STRICT;
