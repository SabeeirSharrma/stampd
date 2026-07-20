import type { FastifyRequest, FastifyReply } from 'fastify'
import * as db from './db.js'

// ── Session Auth (cookie-based, for web UI) ────────────────────

export async function requireSession(req: FastifyRequest, reply: FastifyReply) {
  const sessionId = req.cookies?.['stampd-session']
  if (!sessionId) {
    return reply.status(401).send({ error: 'Not authenticated' })
  }

  const userId = db.validateSession(sessionId)
  if (!userId) {
    return reply.status(401).send({ error: 'Invalid or expired session' })
  }

  const user = db.getUserById(userId)
  if (!user || user.disabled_at) {
    return reply.status(403).send({ error: 'Account disabled' })
  }

  // Attach user to request (id from session, rest from DB)
  const { id: _dbId, ...userData } = user
  ;(req as any).user = { id: userId, ...userData }
}

// ── Token Auth (Bearer token, for API/programmatic access) ─────

export async function requireToken(req: FastifyRequest, reply: FastifyReply) {
  const authHeader = req.headers.authorization
  if (!authHeader?.startsWith('Bearer ')) {
    return reply.status(401).send({ error: 'Missing or invalid Authorization header' })
  }

  const rawToken = authHeader.slice(7)
  // Hash the token to look it up (tokens stored as hashes)
  const tokenHash = await hashToken(rawToken)
  const tokenRow = db.validateToken(tokenHash)

  if (!tokenRow) {
    return reply.status(401).send({ error: 'Invalid or revoked token' })
  }

  const user = db.getUserById(tokenRow.user_id)
  if (!user || user.disabled_at) {
    return reply.status(403).send({ error: 'Account disabled' })
  }

  const { id: _dbId, ...userData } = user
  ;(req as any).user = { id: tokenRow.user_id, ...userData, tokenScope: 'send' }
}

// ── Require Admin ──────────────────────────────────────────────

export async function requireAdmin(req: FastifyRequest, reply: FastifyReply) {
  const user = (req as any).user
  if (!user?.is_admin) {
    return reply.status(403).send({ error: 'Admin access required' })
  }
}

// ── Either Session or Token ────────────────────────────────────

export async function requireAuth(req: FastifyRequest, reply: FastifyReply) {
  // Try session first, then token
  const sessionId = req.cookies?.['stampd-session']
  if (sessionId) {
    const userId = db.validateSession(sessionId)
    if (userId) {
      const user = db.getUserById(userId)
      if (user && !user.disabled_at) {
        const { id: _dbId, ...userData } = user
        ;(req as any).user = { id: userId, ...userData }
        return
      }
    }
  }

  // Try token
  const authHeader = req.headers.authorization
  if (authHeader?.startsWith('Bearer ')) {
    const rawToken = authHeader.slice(7)
    const tokenHash = await hashToken(rawToken)
    const tokenRow = db.validateToken(tokenHash)
    if (tokenRow) {
      const user = db.getUserById(tokenRow.user_id)
      if (user && !user.disabled_at) {
        const { id: _dbId, ...userData } = user
        ;(req as any).user = { id: tokenRow.user_id, ...userData, tokenScope: 'send' }
        return
      }
    }
  }

  return reply.status(401).send({ error: 'Authentication required' })
}

// ── Helpers ────────────────────────────────────────────────────

// Simple hash for token lookup (not for password storage)
async function hashToken(token: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(token)
  const hashBuffer = await crypto.subtle.digest('SHA-256', data)
  const hashArray = Array.from(new Uint8Array(hashBuffer))
  return hashArray.map(b => b.toString(16).padStart(2, '0')).join('')
}

export { hashToken }
