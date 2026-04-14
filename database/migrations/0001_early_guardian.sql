CREATE TABLE `sent_sponsorships` (
	`id` text PRIMARY KEY NOT NULL,
	`created` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
CREATE INDEX `sent_sponsorships_created_idx` ON `sent_sponsorships` (`created`);