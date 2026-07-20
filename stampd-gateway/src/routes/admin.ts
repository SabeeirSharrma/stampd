import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import * as db from '../db.js'

async function requireAdmin(req: FastifyRequest, reply: FastifyReply) {
  const sessionId = req.cookies?.['stampd-session']
  if (sessionId) {
    const userId = db.validateSession(sessionId)
    if (userId) {
      const user = db.getUserById(userId)
      if (user && !user.disabled_at) {
        if (!user.is_admin) {
          return reply.status(403).send({ error: 'Admin access required' })
        }
        const { id: _, ...userData } = user
        ;(req as any).user = { id: userId, ...userData }
        return
      }
    }
  }

  const authHeader = req.headers.authorization
  if (authHeader?.startsWith('Bearer ')) {
    const rawToken = authHeader.slice(7)
    const tokenHash = await hashTokenSimple(rawToken)
    const tokenRow = db.validateToken(tokenHash)
    if (tokenRow) {
      const user = db.getUserById(tokenRow.user_id)
      if (user && !user.disabled_at) {
        if (!user.is_admin) {
          return reply.status(403).send({ error: 'Admin access required' })
        }
        const { id: _, ...userData } = user
        ;(req as any).user = { id: tokenRow.user_id, ...userData, tokenScope: 'send' }
        return
      }
    }
  }

  return reply.status(401).send({ error: 'Authentication required' })
}

async function hashTokenSimple(token: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(token)
  const hashBuffer = await crypto.subtle.digest('SHA-256', data)
  return Array.from(new Uint8Array(hashBuffer)).map(b => b.toString(16).padStart(2, '0')).join('')
}

export default async function adminRoutes(app: FastifyInstance) {
  // ── GET /admin/users ───────────────────────────────────────────
  app.get('/admin/users', { preHandler: requireAdmin }, async () => {
    return db.listUsers()
  })

  // ── DELETE /admin/users/:id ────────────────────────────────────
  app.delete<{ Params: { id: string } }>('/admin/users/:id', { preHandler: requireAdmin }, async (req, reply) => {
    const user = (req as any).user

    const targetId = parseInt(req.params.id)
    if (isNaN(targetId)) {
      return reply.status(400).send({ error: 'Invalid user id' })
    }

    if (targetId === user.id) {
      return reply.status(400).send({ error: 'Cannot delete yourself' })
    }

    const target = db.getUserById(targetId)
    if (!target) {
      return reply.status(404).send({ error: 'User not found' })
    }

    // TODO: cascade delete (sessions, tokens) — handled by FK constraints
    const success = db.revokeToken(targetId) // placeholder — need proper delete
    return { ok: true }
  })

  // ── GET /admin/tokens ──────────────────────────────────────────
  app.get('/admin/tokens', { preHandler: requireAdmin }, async () => {
    return db.listAllTokens()
  })

  // ── DELETE /admin/tokens/:id ───────────────────────────────────
  app.delete<{ Params: { id: string } }>('/admin/tokens/:id', { preHandler: requireAdmin }, async (req, reply) => {
    const tokenId = parseInt(req.params.id)
    if (isNaN(tokenId)) {
      return reply.status(400).send({ error: 'Invalid token id' })
    }

    const revoked = db.revokeToken(tokenId)
    if (!revoked) {
      return reply.status(404).send({ error: 'Token not found or already revoked' })
    }

    return { ok: true }
  })

  // ── GET /admin/config ──────────────────────────────────────────
  app.get('/admin/config', { preHandler: requireAdmin }, async () => {
    return db.getServerConfig()
  })

  // ── GET /admin/queue ───────────────────────────────────────────
  app.get('/admin/queue', { preHandler: requireAdmin }, async () => {
    return db.getQueueStats()
  })

  // ── GET /admin/logs ────────────────────────────────────────────
  app.get('/admin/logs', { preHandler: requireAdmin }, async (req) => {
    const limit = parseInt((req.query as any)?.limit) || 50
    return db.getDeliveryLogs(limit)
  })
}
