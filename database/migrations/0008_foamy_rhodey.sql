CREATE TABLE `sponsor_providers` (
	`provider` text PRIMARY KEY NOT NULL,
	`seeded` integer DEFAULT (strftime('%s','now')) NOT NULL
);
--> statement-breakpoint
INSERT INTO `sponsor_providers` (`provider`, `seeded`) SELECT `provider`, MIN(`created`) FROM `sent_sponsorships` GROUP BY `provider`;
