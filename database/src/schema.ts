import { isNotNull, sql } from "drizzle-orm"
import { index, integer, text, sqliteTable, uniqueIndex } from "drizzle-orm/sqlite-core"

export const githubMessages = sqliteTable('github_messages', {
	id: integer('id').primaryKey().notNull(),
	repositoryId: integer('repository_id').notNull(),
	messageId: integer('message_id').notNull(),

	commits: text('commits', { mode: 'json' }).notNull(),

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
