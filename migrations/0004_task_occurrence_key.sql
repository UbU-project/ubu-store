CREATE UNIQUE INDEX idx_objects_task_occurrence_key
  ON objects(json_extract(payload_json, '$.occurrence.key'))
  WHERE object_type = 'Task' AND json_extract(payload_json, '$.occurrence.key') IS NOT NULL;
