CREATE TABLE `linked_accounts` (
	`id` integer PRIMARY KEY AUTOINCREMENT NOT NULL,
	`provider` text NOT NULL,
	`provider_id` text NOT NULL,
	`login` text,
	`discord_user_id` integer,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE UNIQUE INDEX `linked_accounts_provider_id_idx` ON `linked_accounts` (`provider`,`provider_id`);--> statement-breakpoint
CREATE INDEX `linked_accounts_discord_user_id_idx` ON `linked_accounts` (`discord_user_id`);--> statement-breakpoint
INSERT INTO `linked_accounts` (`provider`, `provider_id`, `login`, `discord_user_id`, `created`) SELECT 'github', CAST(`id` AS TEXT), `login`, `discord_user_id`, `created` FROM `github_users`;--> statement-breakpoint
PRAGMA foreign_keys=OFF;--> statement-breakpoint
CREATE TABLE `__new_sent_sponsorships` (
	`id` text PRIMARY KEY NOT NULL,
	`provider` text DEFAULT 'github' NOT NULL,
	`account_id` integer,
	`amount` integer,
	`message_id` integer,
	`recurring` integer DEFAULT false NOT NULL,
	`ended` integer DEFAULT false NOT NULL,
	`paid` integer,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL,
	FOREIGN KEY (`account_id`) REFERENCES `linked_accounts`(`id`) ON UPDATE no action ON DELETE set null
);
--> statement-breakpoint
INSERT INTO `__new_sent_sponsorships`("id", "provider", "account_id", "amount", "message_id", "recurring", "ended", "paid", "created") SELECT `sent_sponsorships`.`id`, 'github', `linked_accounts`.`id`, `sent_sponsorships`.`amount`, `sent_sponsorships`.`message_id`, `sent_sponsorships`.`recurring`, `sent_sponsorships`.`ended`, `sent_sponsorships`.`paid`, `sent_sponsorships`.`created` FROM `sent_sponsorships` LEFT JOIN `linked_accounts` ON `linked_accounts`.`provider` = 'github' AND `linked_accounts`.`provider_id` = CAST(`sent_sponsorships`.`github_id` AS TEXT);--> statement-breakpoint
DROP TABLE `sent_sponsorships`;--> statement-breakpoint
ALTER TABLE `__new_sent_sponsorships` RENAME TO `sent_sponsorships`;--> statement-breakpoint
PRAGMA foreign_keys=ON;--> statement-breakpoint
CREATE INDEX `sent_sponsorships_created_idx` ON `sent_sponsorships` (`created`);--> statement-breakpoint
CREATE INDEX `sent_sponsorships_provider_idx` ON `sent_sponsorships` (`provider`);--> statement-breakpoint
CREATE UNIQUE INDEX `sent_sponsorships_message_id_idx` ON `sent_sponsorships` (`message_id`) WHERE "sent_sponsorships"."message_id" is not null;--> statement-breakpoint
DROP TABLE `github_users`;
