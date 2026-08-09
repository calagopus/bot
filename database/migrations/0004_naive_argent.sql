ALTER TABLE `sent_sponsorships` ADD `message_id` integer;--> statement-breakpoint
ALTER TABLE `sent_sponsorships` ADD `recurring` integer DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE `sent_sponsorships` ADD `ended` integer DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE `sent_sponsorships` ADD `paid` integer;--> statement-breakpoint
CREATE UNIQUE INDEX `sent_sponsorships_message_id_idx` ON `sent_sponsorships` (`message_id`) WHERE "sent_sponsorships"."message_id" is not null;