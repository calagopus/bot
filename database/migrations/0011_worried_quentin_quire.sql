CREATE TABLE `sponsor_balances` (
	`discord_user_id` integer PRIMARY KEY NOT NULL,
	`available_cents` integer DEFAULT 0 NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE TABLE `sponsor_section_requests` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`discord_user_id` integer NOT NULL,
	`tier_cents` integer NOT NULL,
	`name` text NOT NULL,
	`description` text,
	`logo_path` text,
	`link` text,
	`message_id` integer,
	`approved` integer DEFAULT false NOT NULL,
	`decided` integer,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `sponsor_section_requests_pending_idx` ON `sponsor_section_requests` (`discord_user_id`) WHERE "sponsor_section_requests"."decided" is null;--> statement-breakpoint
CREATE TABLE `sponsor_sections` (
	`discord_user_id` integer PRIMARY KEY NOT NULL,
	`tier_cents` integer NOT NULL,
	`name` text NOT NULL,
	`description` text,
	`logo_path` text,
	`link` text,
	`active` integer DEFAULT false NOT NULL,
	`renewal_warned` integer DEFAULT false NOT NULL,
	`period_start` integer NOT NULL,
	`period_end` integer NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE INDEX `sponsor_sections_active_idx` ON `sponsor_sections` (`active`);--> statement-breakpoint
ALTER TABLE `linked_accounts` ADD `lifetime_cents` integer DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE `linked_accounts` ADD `refunded_cents` integer DEFAULT 0 NOT NULL;