CREATE TABLE `github_messages` (
	`id` integer PRIMARY KEY NOT NULL,
	`repository_id` integer NOT NULL,
	`message_id` integer NOT NULL,
	`commits` text NOT NULL,
	`workflow_sha` text NOT NULL,
	`workflow_status` text NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE INDEX `github_messages_repository_id_idx` ON `github_messages` (`repository_id`);--> statement-breakpoint
CREATE TABLE `text_messages` (
	`id` integer PRIMARY KEY NOT NULL,
	`channel_id` integer NOT NULL,
	`message_id` integer,
	`title` text NOT NULL,
	`content` text NOT NULL,
	`roles` text NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `text_messages_message_id_idx` ON `text_messages` (`message_id`) WHERE "text_messages"."message_id" is not null;