CREATE TABLE `github_users` (
	`id` integer PRIMARY KEY NOT NULL,
	`login` text,
	`discord_user_id` integer,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `github_users_discord_user_id_idx` ON `github_users` (`discord_user_id`) WHERE "github_users"."discord_user_id" is not null;--> statement-breakpoint
INSERT INTO `github_users` (`id`) SELECT DISTINCT `github_id` FROM `sent_sponsorships` WHERE `github_id` IS NOT NULL;--> statement-breakpoint
PRAGMA foreign_keys=OFF;--> statement-breakpoint
CREATE TABLE `__new_sent_sponsorships` (
	`id` text PRIMARY KEY NOT NULL,
	`github_id` integer,
	`amount` integer,
	`message_id` integer,
	`recurring` integer DEFAULT false NOT NULL,
	`ended` integer DEFAULT false NOT NULL,
	`paid` integer,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL,
	FOREIGN KEY (`github_id`) REFERENCES `github_users`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
INSERT INTO `__new_sent_sponsorships`("id", "github_id", "amount", "message_id", "recurring", "ended", "paid", "created") SELECT "id", "github_id", "amount", "message_id", "recurring", "ended", "paid", "created" FROM `sent_sponsorships`;--> statement-breakpoint
DROP TABLE `sent_sponsorships`;--> statement-breakpoint
ALTER TABLE `__new_sent_sponsorships` RENAME TO `sent_sponsorships`;--> statement-breakpoint
PRAGMA foreign_keys=ON;--> statement-breakpoint
CREATE INDEX `sent_sponsorships_created_idx` ON `sent_sponsorships` (`created`);--> statement-breakpoint
CREATE UNIQUE INDEX `sent_sponsorships_message_id_idx` ON `sent_sponsorships` (`message_id`) WHERE "sent_sponsorships"."message_id" is not null;