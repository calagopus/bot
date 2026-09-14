ALTER TABLE `linked_accounts` ADD `native_lifetime_cents` text;--> statement-breakpoint
ALTER TABLE `linked_accounts` ADD `native_refunded_cents` text DEFAULT '{}' NOT NULL;--> statement-breakpoint
ALTER TABLE `linked_accounts` DROP COLUMN `refunded_cents`;