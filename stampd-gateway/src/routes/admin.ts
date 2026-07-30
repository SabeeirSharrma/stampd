import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'

const ADMIN_URL = process.env.ADMIN_URL || 'http://127.0.0.1:8081'

async function hashToken(token: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(token)
  const hashBuffer = await crypto.subtle.digest('SHA-256', data)
  const hashArray = Array.from(new Uint8Array(hashBuffer))
  return hashArray.map(b => b.toString(16).padStart(2, '0')).join('')
}

async function validateAdmin(
  req: FastifyRequest,
  reply: FastifyReply,
): Promise<{ valid: boolean; is_admin: boolean } | void> {
  const sessionId = req.cookies?.['stampd-session']
  const authHeader = req.headers.authorization
  const token = authHeader?.startsWith('Bearer ') ? authHeader.slice(7) : null

  if (!sessionId && !token) {
    reply.status(401).send({ error: 'Authentication required' })
    return
  }

  try {
    const body: Record<string, string> = {}
    if (sessionId) {
      body.session_id = sessionId
    } else if (token) {
      body.token_hash = await hashToken(token)
    }

    const response = await fetch(`${ADMIN_URL}/auth/validate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    })

    if (!response.ok) {
      reply.status(502).send({ error: 'Admin service unavailable' })
      return
    }

    const result = await response.json() as { valid: boolean; is_admin: boolean }

    if (!result.valid) {
      reply.status(401).send({ error: 'Invalid or expired session' })
      return
    }

    if (!result.is_admin) {
      reply.status(403).send({ error: 'Admin access required' })
      return
    }

    return result
  } catch {
    reply.status(502).send({ error: 'Admin service unavailable' })
    return
  }
}

async function requireAdmin(req: FastifyRequest, reply: FastifyReply) {
  await validateAdmin(req, reply)
}

async function proxyToAdmin(
  req: FastifyRequest,
  reply: FastifyReply,
  method: string,
  path: string,
  body?: any,
) {
  try {
    const headers: Record<string, string> = {
      'Content-Type': 'application/json',
    }

    const sessionId = req.cookies?.['stampd-session']
    const authHeader = req.headers.authorization
    if (sessionId) {
      headers['x-stampd-session'] = sessionId
    } else if (authHeader?.startsWith('Bearer ')) {
      headers['x-stampd-token'] = authHeader.slice(7)
    }

    const response = await fetch(`${ADMIN_URL}${path}`, {
      method,
      headers,
      body: body ? JSON.stringify(body) : undefined,
    })

    const data = await response.json()
    return reply.status(response.status).send(data)
  } catch {
    return reply.status(502).send({ error: 'Admin service unavailable' })
  }
}

export default async function adminRoutes(app: FastifyInstance) {
  // ── User routes ────────────────────────────────────────────────

  app.get('/admin/users', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/users')
  })

  app.patch<{ Params: { id: string } }>('/admin/users/:id/disable', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'PATCH', `/admin/users/${req.params.id}/disable`)
  })

  app.delete<{ Params: { id: string } }>('/admin/users/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'DELETE', `/admin/users/${req.params.id}`)
  })

  app.get<{ Params: { id: string } }>('/admin/users/:id/tokens', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', `/admin/users/${req.params.id}/tokens`)
  })

  // ── Token routes ───────────────────────────────────────────────

  app.get('/admin/tokens', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/tokens')
  })

  app.get('/admin/tokens/stats', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/tokens/stats')
  })

  app.delete<{ Params: { id: string } }>('/admin/tokens/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'DELETE', `/admin/tokens/${req.params.id}`)
  })

  // ── Config routes ──────────────────────────────────────────────

  app.get('/admin/config', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/config')
  })

  app.patch('/admin/config', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'PATCH', '/admin/config', req.body)
  })

  // ── Queue routes ───────────────────────────────────────────────

  app.get('/admin/queue', { preHandler: requireAdmin }, async (req, reply) => {
    const status = (req.query as any)?.status
    const query = status ? `?status=${status}` : ''
    return proxyToAdmin(req, reply, 'GET', `/admin/queue${query}`)
  })

  app.post<{ Params: { id: string } }>('/admin/queue/:id/retry', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'POST', `/admin/queue/${req.params.id}/retry`)
  })

  app.delete<{ Params: { id: string } }>('/admin/queue/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'DELETE', `/admin/queue/${req.params.id}`)
  })

  // ── Log routes ─────────────────────────────────────────────────

  app.get('/admin/logs', { preHandler: requireAdmin }, async (req, reply) => {
    const { status, recipient, limit } = req.query as any
    const params = new URLSearchParams()
    if (status) params.set('status', status)
    if (recipient) params.set('recipient', recipient)
    if (limit) params.set('limit', limit)
    const query = params.toString() ? `?${params.toString()}` : ''
    return proxyToAdmin(req, reply, 'GET', `/admin/logs${query}`)
  })

  // ── Filter routes ──────────────────────────────────────────────

  app.get('/admin/filters', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/filters')
  })

  app.get<{ Params: { id: string } }>('/admin/filters/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', `/admin/filters/${req.params.id}`)
  })

  app.post('/admin/filters', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'POST', '/admin/filters', req.body)
  })

  app.patch<{ Params: { id: string } }>('/admin/filters/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'PATCH', `/admin/filters/${req.params.id}`, req.body)
  })

  app.delete<{ Params: { id: string } }>('/admin/filters/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'DELETE', `/admin/filters/${req.params.id}`)
  })

  // ── Domain routes ──────────────────────────────────────────────

  app.get('/admin/domains', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'GET', '/admin/domains')
  })

  app.post<{ Body: { domain: string } }>('/admin/domains', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'POST', '/admin/domains', req.body)
  })

  app.post<{ Body: { id: number } }>('/admin/domains/verify', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'POST', '/admin/domains/verify', req.body)
  })

  app.delete<{ Params: { id: string } }>('/admin/domains/:id', { preHandler: requireAdmin }, async (req, reply) => {
    return proxyToAdmin(req, reply, 'DELETE', `/admin/domains/${req.params.id}`)
  })

  // ── Quota route ────────────────────────────────────────────────

  app.get('/admin/quota', { preHandler: requireAdmin }, async (req, reply) => {
    // TODO: proxy to admin service when quota endpoint is implemented
    return reply.status(501).send({ error: 'Not implemented' })
  })
}
