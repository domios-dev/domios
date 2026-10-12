-- Captured from isolated OpenCode 1.19.0 import; synthetic payload, paths redacted.
CREATE TABLE `project` (
          `id` text PRIMARY KEY,
          `worktree` text NOT NULL,
          `vcs` text,
          `name` text,
          `icon_url` text,
          `icon_url_override` text,
          `icon_color` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `time_initialized` integer,
          `sandboxes` text NOT NULL,
          `commands` text
        );
INSERT INTO project (id, worktree, vcs, name, icon_url, icon_url_override, icon_color, time_created, time_updated, time_initialized, sandboxes, commands) VALUES ('project_fixture', '/fixture', 'git', NULL, NULL, NULL, NULL, 1791709649812, 1791709649812, NULL, '[]', NULL);
CREATE TABLE `session` (
          `id` text PRIMARY KEY,
          `project_id` text NOT NULL,
          `workspace_id` text,
          `parent_id` text,
          `slug` text NOT NULL,
          `directory` text NOT NULL,
          `path` text,
          `title` text NOT NULL,
          `version` text NOT NULL,
          `share_url` text,
          `summary_additions` integer,
          `summary_deletions` integer,
          `summary_files` integer,
          `summary_diffs` text,
          `metadata` text,
          `cost` real DEFAULT 0 NOT NULL,
          `tokens_input` integer DEFAULT 0 NOT NULL,
          `tokens_output` integer DEFAULT 0 NOT NULL,
          `tokens_reasoning` integer DEFAULT 0 NOT NULL,
          `tokens_cache_read` integer DEFAULT 0 NOT NULL,
          `tokens_cache_write` integer DEFAULT 0 NOT NULL,
          `revert` text,
          `permission` text,
          `agent` text,
          `model` text,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `time_compacting` integer,
          `time_archived` integer,
          CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
        );
INSERT INTO session (id, project_id, workspace_id, parent_id, slug, directory, path, title, version, share_url, summary_additions, summary_deletions, summary_files, summary_diffs, metadata, cost, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write, revert, permission, agent, model, time_created, time_updated, time_compacting, time_archived) VALUES ('ses_fixture_owned', 'project_fixture', NULL, NULL, 'fixture', '/fixture', '', 'fixture', '1.19.0', NULL, NULL, NULL, NULL, NULL, NULL, 0.0, 0, 0, 0, 0, 0, NULL, NULL, NULL, NULL, 1000, 2000, NULL, NULL);
CREATE TABLE `message` (
          `id` text PRIMARY KEY,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
        );
INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_fixture_user', 'ses_fixture_owned', 1000, 1791709649980, '{"role":"user","time":{"created":1000},"agent":"build","model":{"providerID":"fixture","modelID":"fixture-model"}}');
INSERT INTO message (id, session_id, time_created, time_updated, data) VALUES ('msg_fixture_assistant', 'ses_fixture_owned', 2000, 1791709649981, '{"role":"assistant","time":{"created":2000},"parentID":"msg_fixture_user","modelID":"fixture-model","providerID":"fixture","mode":"build","agent":"build","path":{"cwd":"/fixture","root":"/fixture"},"cost":0,"tokens":{"input":10,"output":5,"reasoning":0,"cache":{"read":0,"write":0}}}');
CREATE TABLE `part` (
          `id` text PRIMARY KEY,
          `message_id` text NOT NULL,
          `session_id` text NOT NULL,
          `time_created` integer NOT NULL,
          `time_updated` integer NOT NULL,
          `data` text NOT NULL,
          CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE
        );
INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_fixture_user', 'msg_fixture_user', 'ses_fixture_owned', 1791709649981, 1791709649981, '{"type":"text","text":"opaque-user-turn"}');
INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_fixture_assistant', 'msg_fixture_assistant', 'ses_fixture_owned', 1791709649981, 1791709649981, '{"type":"text","text":"opaque-assistant-turn"}');
INSERT INTO part (id, message_id, session_id, time_created, time_updated, data) VALUES ('prt_fixture_tool', 'msg_fixture_assistant', 'ses_fixture_owned', 1791709649981, 1791709649981, '{"type":"tool","callID":"call_fixture","tool":"bash","state":{"status":"completed","input":{"command":"printf fixture"},"output":"fixture","title":"fixture","metadata":{},"time":{"start":2000,"end":2001}}}');
