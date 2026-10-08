-- Schema observed from OpenCode 1.18.35; synthetic session content.
CREATE TABLE `project` (
	`id` text PRIMARY KEY,
	`worktree` text NOT NULL,
	`vcs` text,
	`name` text,
	`icon_url` text,
	`icon_color` text,
	`time_created` integer NOT NULL,
	`time_updated` integer NOT NULL,
	`time_initialized` integer,
	`sandboxes` text NOT NULL
, `commands` text, `icon_url_override` text);
CREATE TABLE `message` (
	`id` text PRIMARY KEY,
	`session_id` text NOT NULL,
	`time_created` integer NOT NULL,
	`time_updated` integer NOT NULL,
	`data` text NOT NULL,
	CONSTRAINT `fk_message_session_id_session_id_fk` FOREIGN KEY (`session_id`) REFERENCES `session`(`id`) ON DELETE CASCADE
);
CREATE TABLE `part` (
	`id` text PRIMARY KEY,
	`message_id` text NOT NULL,
	`session_id` text NOT NULL,
	`time_created` integer NOT NULL,
	`time_updated` integer NOT NULL,
	`data` text NOT NULL,
	CONSTRAINT `fk_part_message_id_message_id_fk` FOREIGN KEY (`message_id`) REFERENCES `message`(`id`) ON DELETE CASCADE
);
CREATE TABLE `session` (
	`id` text PRIMARY KEY,
	`project_id` text NOT NULL,
	`parent_id` text,
	`slug` text NOT NULL,
	`directory` text NOT NULL,
	`title` text NOT NULL,
	`version` text NOT NULL,
	`share_url` text,
	`summary_additions` integer,
	`summary_deletions` integer,
	`summary_files` integer,
	`summary_diffs` text,
	`revert` text,
	`permission` text,
	`time_created` integer NOT NULL,
	`time_updated` integer NOT NULL,
	`time_compacting` integer,
	`time_archived` integer, `workspace_id` text, `path` text, `agent` text, `model` text, `cost` real DEFAULT 0 NOT NULL, `tokens_input` integer DEFAULT 0 NOT NULL, `tokens_output` integer DEFAULT 0 NOT NULL, `tokens_reasoning` integer DEFAULT 0 NOT NULL, `tokens_cache_read` integer DEFAULT 0 NOT NULL, `tokens_cache_write` integer DEFAULT 0 NOT NULL, `metadata` text,
	CONSTRAINT `fk_session_project_id_project_id_fk` FOREIGN KEY (`project_id`) REFERENCES `project`(`id`) ON DELETE CASCADE
);
INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) VALUES ('project_fixture', '/fixture', 1000, 2000, '[]');
INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated)
VALUES ('ses_fixture_owned', 'project_fixture', 'fixture', '/fixture', 'fixture', '1.18.35', 1000, 2000),
       ('ses_foreign', 'project_fixture', 'foreign', '/foreign', 'foreign', '1.18.35', 1000, 2000);
INSERT INTO message (id, session_id, time_created, time_updated, data)
VALUES ('msg_fixture_user', 'ses_fixture_owned', 1000, 1000, '{"role":"user"}'),
       ('msg_fixture_assistant', 'ses_fixture_owned', 2000, 2000, '{"role":"assistant","providerID":"fixture","modelID":"fixture-model"}'),
       ('msg_fixture_foreign', 'ses_foreign', 3000, 3000, '{"role":"user"}');
INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
VALUES ('prt_fixture_user', 'msg_fixture_user', 'ses_fixture_owned', 1000, 1000, '{"type":"text","text":"opaque-user-turn"}'),
       ('prt_fixture_assistant', 'msg_fixture_assistant', 'ses_fixture_owned', 2000, 2000, '{"type":"text","text":"opaque-assistant-turn"}'),
       ('prt_fixture_foreign', 'msg_fixture_foreign', 'ses_foreign', 3000, 3000, '{"type":"text","text":"foreign-turn"}');
