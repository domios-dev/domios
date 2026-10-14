-- Mirrors the live opencode.db shape: usage lives in message.data as a JSON blob.
CREATE TABLE session (
  id TEXT PRIMARY KEY,
  directory TEXT,
  title TEXT,
  time_created INTEGER,
  time_updated INTEGER
);

CREATE TABLE message (
  id TEXT PRIMARY KEY,
  session_id TEXT,
  time_created INTEGER,
  time_updated INTEGER,
  data TEXT
);

INSERT INTO session VALUES ('ses_1', '/repo', 'kpi lane', 1784633000000, 1784633090000);

INSERT INTO message VALUES ('msg_1', 'ses_1', 1784633070000, 1784633070000,
  '{"role":"user","time":{"created":1784633070000}}');

INSERT INTO message VALUES ('msg_2', 'ses_1', 1784633081000, 1784633087961,
  '{"role":"assistant","modelID":"big-pickle","providerID":"opencode","cost":0,"tokens":{"total":23895,"input":279,"output":73,"reasoning":247,"cache":{"read":23296,"write":0}},"time":{"created":1784633081000,"completed":1784633087961}}');

INSERT INTO message VALUES ('msg_3', 'ses_1', 1784633090000, 1784633095000,
  '{"role":"assistant","modelID":"big-pickle","providerID":"opencode","cost":0,"tokens":{"total":32221,"input":21,"output":400,"reasoning":100,"cache":{"read":30000,"write":1700}},"time":{"created":1784633090000,"completed":1784633095000}}');
