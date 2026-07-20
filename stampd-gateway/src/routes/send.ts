import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import * as db from '../db.js'
import { writeFile, mkdir } from 'node:fs/promises'
import { join } from 'node:path'

async function requireAuth(req: FastifyRequest, reply: FastifyReply) {
  const sessionId = req.cookies?.['stampd-session']
  if (sessionId) {
    const userId = db.validateSession(sessionId)
    if (userId) {
      const user = db.getUserById(userId)
      if (user && !user.disabled_at) {
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

export default async function sendRoutes(app: FastifyInstance) {
  // ── POST /messages/send ────────────────────────────────────────
  app.post<{
    Body: {
      to: string
      subject?: string
      body: string
      from?: string
    }
  }>('/messages/send', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user

    const { to, subject, body, from } = req.body || {}
    if (!to || !body) {
      return reply.status(400).send({ error: 'Recipient and body are required' })
    }

    // Validate recipient is external (not our domain)
    const config = db.getServerConfig()
    const recipientDomain = to.split('@')[1]
    if (recipientDomain === config.domain) {
      return reply.status(400).send({ error: 'Cannot send to local users via API — use the mailbox' })
    }

    // Build the .eml message
    const sender = from || user.email
    const messageId = `<${crypto.randomUUID()}@${config.domain}>`
    const now = new Date().toUTCString()

    const eml = [
      `From: ${sender}`,
      `To: ${to}`,
      `Subject: ${subject || '(no subject)'}`,
      `Date: ${now}`,
      `Message-ID: ${messageId}`,
      `MIME-Version: 1.0`,
      `Content-Type: text/plain; charset=utf-8`,
      '',
      body,
    ].join('\r\n')

    // Save to outbox
    const outboxDir = process.env.STAMPD_OUTBOX || '/var/lib/stampd/outbox'
    await mkdir(outboxDir, { recursive: true })

    const timestamp = Math.floor(Date.now() / 1000)
    const msgFilename = `out-${timestamp}-${crypto.randomUUID().slice(0, 8)}.eml`
    const msgPath = join(outboxDir, msgFilename)
    await writeFile(msgPath, eml)

    // Enqueue for delivery
    const queueId = db.enqueueMessage(user.id, to, msgPath)

    return reply.status(202).send({
      id: queueId,
      status: 'queued',
      recipient: to,
    })
  })
}
