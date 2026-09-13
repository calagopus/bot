import { isNotNull, isNull, sql } from "drizzle-orm"
import { index, integer, text, sqliteTable, uniqueIndex } from "drizzle-orm/sqlite-core"

export const githubMessages = sqliteTable('github_messages', {
	id: integer('id').primaryKey().notNull(),
	repositoryId: integer('repository_id').notNull(),
	messageId: integer('message_id').notNull(),

	commits: text('commits', { mode: 'json' }).notNull(),

	branch: text('branch').default('').notNull(),
	compare: text('compare').default('').notNull(),

	workflowSha: text('workflow_sha').notNull(),
	workflowStatus: text('workflow_status', { mode: 'json' }).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (githubMessages) => [
	index('github_messages_repository_id_idx').on(githubMessages.repositoryId)
])

export const textMessages = sqliteTable('text_messages', {
	id: integer('id').primaryKey().notNull(),
	channelId: integer('channel_id').notNull(),
	messageId: integer('message_id'),

	title: text('title').notNull(),
	content: text('content').notNull(),

	roles: text('roles', { mode: 'json' }).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (textMessages) => [
	uniqueIndex('text_messages_message_id_idx').on(textMessages.messageId).where(isNotNull(textMessages.messageId))
])

export const linkedAccounts = sqliteTable('linked_accounts', {
	id: integer('id').primaryKey({ autoIncrement: true }).notNull(),

	provider: text('provider').notNull(),
	providerId: text('provider_id').notNull(),

	login: text('login'),

	discordUserId: integer('discord_user_id'),

	lifetimeCents: integer('lifetime_cents').default(0).notNull(),
	refundedCents: integer('refunded_cents').default(0).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (linkedAccounts) => [
	uniqueIndex('linked_accounts_provider_id_idx').on(linkedAccounts.provider, linkedAccounts.providerId),
	index('linked_accounts_discord_user_id_idx').on(linkedAccounts.discordUserId)
])

export const sentSponsorships = sqliteTable('sent_sponsorships', {
	id: text('id').primaryKey().notNull(),

	provider: text('provider').default('github').notNull(),
	accountId: integer('account_id').references(() => linkedAccounts.id, { onDelete: 'set null' }),
	amount: integer('amount'),
	currency: text('currency'),

	messageId: integer('message_id'),
	recurring: integer('recurring', { mode: 'boolean' }).default(false).notNull(),
	ended: integer('ended', { mode: 'boolean' }).default(false).notNull(),
	paid: integer('paid'),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (sentSponsorships) => [
	index('sent_sponsorships_created_idx').on(sentSponsorships.created),
	index('sent_sponsorships_provider_idx').on(sentSponsorships.provider),
	uniqueIndex('sent_sponsorships_message_id_idx').on(sentSponsorships.messageId).where(isNotNull(sentSponsorships.messageId))
])

export const sponsorProviders = sqliteTable('sponsor_providers', {
	provider: text('provider').primaryKey().notNull(),

	seeded: integer('seeded', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
})

export const aiSupportThreads = sqliteTable('ai_support_threads', {
	threadId: integer('thread_id').primaryKey().notNull(),
	messageId: integer('message_id'),
	ownerId: integer('owner_id'),

	enabled: integer('enabled', { mode: 'boolean' }).default(false).notNull(),
	paused: integer('paused', { mode: 'boolean' }).default(false).notNull(),
	resolved: integer('resolved', { mode: 'boolean' }).default(false).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
})

export const sponsorBalances = sqliteTable('sponsor_balances', {
	discordUserId: integer('discord_user_id').primaryKey().notNull(),

	availableCents: integer('available_cents').default(0).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
})

export const sponsorSections = sqliteTable('sponsor_sections', {
	discordUserId: integer('discord_user_id').primaryKey().notNull(),

	tierCents: integer('tier_cents').notNull(),

	name: text('name').notNull(),
	description: text('description'),
	logoPath: text('logo_path'),
	link: text('link'),

	active: integer('active', { mode: 'boolean' }).default(false).notNull(),
	renewalWarned: integer('renewal_warned', { mode: 'boolean' }).default(false).notNull(),

	periodStart: integer('period_start', { mode: 'timestamp' }).notNull(),
	periodEnd: integer('period_end', { mode: 'timestamp' }).notNull(),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (sponsorSections) => [
	index('sponsor_sections_active_idx').on(sponsorSections.active)
])

export const sponsorSectionRequests = sqliteTable('sponsor_section_requests', {
	id: integer('id').primaryKey({ autoIncrement: true }).notNull(),

	discordUserId: integer('discord_user_id').notNull(),

	tierCents: integer('tier_cents').notNull(),

	name: text('name').notNull(),
	description: text('description'),
	logoPath: text('logo_path'),
	link: text('link'),

	messageId: integer('message_id'),

	approved: integer('approved', { mode: 'boolean' }).default(false).notNull(),
	decided: integer('decided', { mode: 'timestamp' }),

	created: integer('created', { mode: 'timestamp' }).default(sql`(strftime('%s','now'))`).notNull(),
}, (sponsorSectionRequests) => [
	uniqueIndex('sponsor_section_requests_pending_idx').on(sponsorSectionRequests.discordUserId).where(isNull(sponsorSectionRequests.decided))
])
