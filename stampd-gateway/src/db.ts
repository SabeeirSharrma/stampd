import { Database } from 'bun:sqlite'

let db: Database | null = null

export function getDb(): Database {
  if (!db) {
    const dbPath = process.env.STAMPD_DB_PATH || '/var/lib/stampd/stampd.db'
    db = new Database(dbPath)
    db.run('PRAGMA journal_mode = WAL')
    db.run('PRAGMA foreign_keys = ON')
  }
  return db
}

// ── Server Config ──────────────────────────────────────────────

export function getServerConfig() {
  return getDb().query(
    'SELECT domain, signup_enabled, dkim_selector FROM server_config WHERE id = 1'
  ).get() as { domain: string; signup_enabled: number; dkim_selector: string }
}

// ── Users ──────────────────────────────────────────────────────

export interface UserRow {
  id: number
  email: string
  password_hash: string
  is_admin: number
  disabled_at: number | null
}

export function getUserByEmail(email: string): UserRow | undefined {
  return getDb().query(
    'SELECT id, email, password_hash, is_admin, disabled_at FROM users WHERE email = ?'
  ).get(email) as UserRow | undefined
}

export function getUserById(id: number): Omit<UserRow, 'password_hash'> | undefined {
  return getDb().query(
    'SELECT id, email, is_admin, disabled_at FROM users WHERE id = ?'
  ).get(id) as Omit<UserRow, 'password_hash'> | undefined
}

export function createUser(email: string, passwordHash: string, isAdmin = false): number {
  const now = Math.floor(Date.now() / 1000)
  const result = getDb().run(
    'INSERT INTO users (email, password_hash, is_admin, created_at) VALUES (?, ?, ?, ?)',
    [email, passwordHash, isAdmin ? 1 : 0, now]
  )
  return Number(result.lastInsertRowid)
}

export function listUsers() {
  return getDb().query(
    'SELECT id, email, is_admin, disabled_at IS NOT NULL as disabled FROM users ORDER BY id'
  ).all()
}

// ── Tokens ─────────────────────────────────────────────────────

export interface TokenRow {
  id: number
  user_id: number
  token_hash: string
  label: string
  scope: string
  created_at: number
  last_used_at: number | null
  revoked_at: number | null
}

export function createToken(userId: number, tokenHash: string, label: string, scope = 'send'): number {
  const now = Math.floor(Date.now() / 1000)
  const result = getDb().run(
    'INSERT INTO auth_tokens (user_id, token_hash, label, scope, created_at) VALUES (?, ?, ?, ?, ?)',
    [userId, tokenHash, label, scope, now]
  )
  return Number(result.lastInsertRowid)
}

export function validateToken(tokenHash: string): { token_id: number; user_id: number } | undefined {
  const row = getDb().query(
    'SELECT id as token_id, user_id FROM auth_tokens WHERE token_hash = ? AND revoked_at IS NULL'
  ).get(tokenHash) as { token_id: number; user_id: number } | undefined

  if (row) {
    const now = Math.floor(Date.now() / 1000)
    getDb().run('UPDATE auth_tokens SET last_used_at = ? WHERE id = ?', [now, row.token_id])
  }
  return row
}

export function listUserTokens(userId: number) {
  return getDb().query(
    'SELECT id, label, scope, created_at, last_used_at, revoked_at IS NOT NULL as revoked FROM auth_tokens WHERE user_id = ? ORDER BY id'
  ).all(userId)
}

export function listAllTokens() {
  return getDb().query(
    'SELECT id, user_id, label, scope, created_at, revoked_at IS NOT NULL as revoked FROM auth_tokens ORDER BY id'
  ).all()
}

export function revokeToken(id: number): boolean {
  const now = Math.floor(Date.now() / 1000)
  const result = getDb().run(
    'UPDATE auth_tokens SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL',
    [now, id]
  )
  return result.changes > 0
}

// ── Sessions ───────────────────────────────────────────────────

export function createSession(userId: number, expiresAt: number): string {
  const id = crypto.randomUUID()
  getDb().run(
    'INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?, ?, ?, ?)',
    [id, userId, Math.floor(Date.now() / 1000), expiresAt]
  )
  return id
}

export function validateSession(sessionId: string): number | undefined {
  const now = Math.floor(Date.now() / 1000)
  const row = getDb().query(
    'SELECT user_id FROM sessions WHERE id = ? AND expires_at > ?'
  ).get(sessionId, now) as { user_id: number } | undefined
  return row?.user_id
}

export function deleteSession(sessionId: string): boolean {
  const result = getDb().run('DELETE FROM sessions WHERE id = ?', [sessionId])
  return result.changes > 0
}

// ── Delivery Queue ─────────────────────────────────────────────

export function enqueueMessage(
  fromUserId: number,
  recipient: string,
  messagePath: string
): number {
  const now = Math.floor(Date.now() / 1000)
  const result = getDb().run(
    "INSERT INTO delivery_queue (from_user_id, recipient, message_path, attempts, next_attempt_at, status) VALUES (?, ?, ?, 0, ?, 'pending')",
    [fromUserId, recipient, messagePath, now]
  )
  return Number(result.lastInsertRowid)
}

export function getQueueStats() {
  const pending = (getDb().query("SELECT COUNT(*) as c FROM delivery_queue WHERE status = 'pending'").get() as { c: number }).c
  const delivered = (getDb().query("SELECT COUNT(*) as c FROM delivery_queue WHERE status = 'delivered'").get() as { c: number }).c
  const dead = (getDb().query("SELECT COUNT(*) as c FROM delivery_queue WHERE status = 'dead'").get() as { c: number }).c
  return { pending, delivered, dead }
}

// ── Delivery Logs ──────────────────────────────────────────────

export function getDeliveryLogs(limit = 50) {
  return getDb().query(
    'SELECT id, queue_id, status, recipient, error, created_at FROM delivery_logs ORDER BY id DESC LIMIT ?'
  ).all(limit)
}
