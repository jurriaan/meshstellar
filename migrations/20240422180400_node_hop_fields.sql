ALTER TABLE nodes ADD COLUMN last_hop_start INTEGER NULL;
ALTER TABLE nodes ADD COLUMN last_hop_limit INTEGER NULL;
UPDATE nodes 
SET last_hop_start = (SELECT hop_start FROM mesh_packets WHERE from_id = nodes.node_id ORDER BY rx_time DESC LIMIT 1),
    last_hop_limit = (SELECT hop_limit FROM mesh_packets WHERE from_id = nodes.node_id ORDER BY rx_time DESC LIMIT 1);
