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

  // ── PATCH /admin/users/:id/disable ─────────────────────────────
  app.patch<{ Params: { id: string } }>('/admin/users/:id/disable', { preHandler: requireAdmin }, async (req, reply) => {
    const user = (req as any).user
    const targetId = parseInt(req.params.id)
    if (isNaN(targetId)) {
      return reply.status(400).send({ error: 'Invalid user id' })
    }
    if (targetId === user.id) {
      return reply.status(400).send({ error: 'Cannot disable yourself' })
    }
    const target = db.getUserById(targetId)
    if (!target) {
      return reply.status(404).send({ error: 'User not found' })
    }
    if (target.disabled_at) {
      return reply.status(400).send({ error: 'User already disabled' })
    }
    const success = db.disableUser(targetId)
    return { ok: success }
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
    // Revoke all user tokens first
    const tokens = db.getUserTokens(targetId) as any[]
    for (const token of tokens) {
      if (!token.revoked) {
        db.revokeToken(token.id)
      }
    }
    const success = db.deleteUser(targetId)
    return { ok: success }
  })

  // ── GET /admin/users/:id/tokens ────────────────────────────────
  app.get<{ Params: { id: string } }>('/admin/users/:id/tokens', { preHandler: requireAdmin }, async (req, reply) => {
    const targetId = parseInt(req.params.id)
    if (isNaN(targetId)) {
      return reply.status(400).send({ error: 'Invalid user id' })
    }
    const target = db.getUserById(targetId)
    if (!target) {
      return reply.status(404).send({ error: 'User not found' })
    }
    return db.getUserTokens(targetId)
  })

  // ── GET /admin/tokens ──────────────────────────────────────────
  app.get('/admin/tokens', { preHandler: requireAdmin }, async () => {
    return db.listAllTokens()
  })

  // ── GET /admin/tokens/stats ────────────────────────────────────
  app.get('/admin/tokens/stats', { preHandler: requireAdmin }, async () => {
    return db.getTokenStats()
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

  // ── PATCH /admin/config ────────────────────────────────────────
  app.patch('/admin/config', { preHandler: requireAdmin }, async (req) => {
    const body = req.body as any
    const updates: any = {}
    if (body.domain !== undefined) updates.domain = body.domain
    if (body.signup_enabled !== undefined) updates.signup_enabled = body.signup_enabled
    if (body.dkim_selector !== undefined) updates.dkim_selector = body.dkim_selector

    if (Object.keys(updates).length === 0) {
      return { ok: false, error: 'No fields to update' }
    }

    const success = db.updateServerConfig(updates)
    return { ok: success, config: db.getServerConfig() }
  })

  // ── GET /admin/quota ───────────────────────────────────────────
  app.get('/admin/quota', { preHandler: requireAdmin }, async () => {
    return db.getQuotaUsage()
  })

  // ── GET /admin/queue ───────────────────────────────────────────
  app.get('/admin/queue', { preHandler: requireAdmin }, async (req) => {
    const status = (req.query as any)?.status as string | undefined
    return db.listQueueMessages(status)
  })

  // ── POST /admin/queue/:id/retry ────────────────────────────────
  app.post<{ Params: { id: string } }>('/admin/queue/:id/retry', { preHandler: requireAdmin }, async (req, reply) => {
    const msgId = parseInt(req.params.id)
    if (isNaN(msgId)) {
      return reply.status(400).send({ error: 'Invalid message id' })
    }
    const retried = db.retryMessage(msgId)
    if (!retried) {
      return reply.status(404).send({ error: 'Message not found or not dead-lettered' })
    }
    return { ok: true }
  })

  // ── DELETE /admin/queue/:id ────────────────────────────────────
  app.delete<{ Params: { id: string } }>('/admin/queue/:id', { preHandler: requireAdmin }, async (req, reply) => {
    const msgId = parseInt(req.params.id)
    if (isNaN(msgId)) {
      return reply.status(400).send({ error: 'Invalid message id' })
    }
    const purged = db.purgeMessage(msgId)
    if (!purged) {
      return reply.status(404).send({ error: 'Message not found' })
    }
    return { ok: true }
  })

  // ── GET /admin/logs ────────────────────────────────────────────
  app.get('/admin/logs', { preHandler: requireAdmin }, async (req) => {
    const { status, recipient, limit } = req.query as any
    return db.getDeliveryLogsFiltered({
      status: status || undefined,
      recipient: recipient || undefined,
      limit: limit ? parseInt(limit) : 50,
    })
  })
}
