-- SQLite does not allow updating the nullability. Manually updating the schema works

PRAGMA writable_schema = 1;

UPDATE SQLITE_MASTER SET SQL = 'CREATE TABLE "environment_metrics" ( "id" integer NOT NULL PRIMARY KEY AUTOINCREMENT, "mesh_packet_id" integer UNIQUE NOT NULL, "node_id" integer NOT NULL, "time" integer, "temperature" real, "relative_humidity" real, "barometric_pressure" real, "gas_resistance" real, iaq INTEGER DEFAULT 0, FOREIGN KEY ("mesh_packet_id") REFERENCES "mesh_packets" ("id") ON DELETE CASCADE ) STRICT' WHERE name = 'environment_metrics';

PRAGMA writable_schema = 0;

