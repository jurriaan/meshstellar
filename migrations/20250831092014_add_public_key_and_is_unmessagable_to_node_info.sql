ALTER TABLE node_info ADD COLUMN public_key BLOB NULL;
ALTER TABLE node_info ADD COLUMN is_unmessagable INTEGER NULL;
ALTER TABLE nodes ADD COLUMN public_key BLOB NULL;
ALTER TABLE nodes ADD COLUMN is_unmessagable INTEGER NULL;
