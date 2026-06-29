import { sql } from 'drizzle-orm'
import { jsonb, pgTable, text, timestamp, uniqueIndex, uuid } from 'drizzle-orm/pg-core'

export const users = pgTable('users', {
  id: uuid('id').primaryKey().defaultRandom(),
  email: text('email').notNull().unique(),
  passwordHash: text('password_hash').notNull(),
  passwordAlgo: text('password_algo').notNull(),
  role: text('role').notNull(),
  status: text('status').notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
  updatedAt: timestamp('updated_at', { withTimezone: true }).notNull(),
  passwordUpdatedAt: timestamp('password_updated_at', { withTimezone: true }).notNull()
})

export const refreshTokens = pgTable('refresh_tokens', {
  id: uuid('id').primaryKey().defaultRandom(),
  userId: uuid('user_id').notNull().references(() => users.id),
  tokenHash: text('token_hash').notNull().unique(),
  clientId: text('client_id').notNull(),
  userAgent: text('user_agent'),
  ip: text('ip'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  revokedAt: timestamp('revoked_at', { withTimezone: true }),
  rotatedFromId: uuid('rotated_from_id'),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull()
})

export const devices = pgTable(
  'devices',
  {
    id: uuid('id').primaryKey().defaultRandom(),
    userId: uuid('user_id').notNull().references(() => users.id),
    name: text('name').notNull(),
    fingerprintHash: text('fingerprint_hash').notNull(),
    tokenHash: text('token_hash').notNull().unique(),
    identityPublicKeyJson: jsonb('identity_public_key_json').notNull(),
    status: text('status').notNull(),
    lastSeenAt: timestamp('last_seen_at', { withTimezone: true }),
    capabilityJson: jsonb('capability_json').notNull(),
    createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
    updatedAt: timestamp('updated_at', { withTimezone: true }).notNull(),
    revokedAt: timestamp('revoked_at', { withTimezone: true })
  },
  (table) => [
    // 与 0001 手写迁移保持一致：只约束 active 设备，允许历史 revoked 记录保留。
    uniqueIndex('devices_active_user_fingerprint_unique')
      .on(table.userId, table.fingerprintHash)
      .where(sql`${table.status} = 'active'`)
  ]
)

export const remoteConnections = pgTable('remote_connections', {
  id: uuid('id').primaryKey().defaultRandom(),
  userId: uuid('user_id').notNull().references(() => users.id),
  deviceId: uuid('device_id').notNull().references(() => devices.id),
  clientId: text('client_id').notNull(),
  status: text('status').notNull(),
  transportPreference: text('transport_preference').notNull(),
  transportSelected: text('transport_selected'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
  connectedAt: timestamp('connected_at', { withTimezone: true }),
  closedAt: timestamp('closed_at', { withTimezone: true }),
  closeReason: text('close_reason')
})

export const desktopLoginSessions = pgTable('desktop_login_sessions', {
  id: uuid('id').primaryKey().defaultRandom(),
  requestId: text('request_id').notNull().unique(),
  pollTokenHash: text('poll_token_hash').notNull().unique(),
  desktopPublicKey: text('desktop_public_key').notNull(),
  deviceIdentityPublicKey: text('device_identity_public_key').notNull(),
  deviceName: text('device_name').notNull(),
  fingerprintHash: text('fingerprint_hash').notNull(),
  capabilityJson: jsonb('capability_json').notNull(),
  status: text('status').notNull(),
  userId: uuid('user_id').references(() => users.id),
  deviceId: uuid('device_id').references(() => devices.id),
  encryptedResultJson: jsonb('encrypted_result_json'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  completedAt: timestamp('completed_at', { withTimezone: true }),
  consumedAt: timestamp('consumed_at', { withTimezone: true }),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull()
})
