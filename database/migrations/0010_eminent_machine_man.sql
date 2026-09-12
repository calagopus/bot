CREATE TABLE `ai_support_threads` (
	`thread_id` integer PRIMARY KEY NOT NULL,
	`message_id` integer,
	`owner_id` integer,
	`enabled` integer DEFAULT false NOT NULL,
	`paused` integer DEFAULT false NOT NULL,
	`resolved` integer DEFAULT false NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
