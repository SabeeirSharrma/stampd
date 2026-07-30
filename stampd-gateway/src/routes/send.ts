import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import * as db from '../db.js'
import { getDb } from '../db.js'
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

function generateFilename(): string {
  const ts = Date.now()
  const seq = Math.floor(Math.random() * 100000)
  const pid = process.pid
  const hostname = require('os').hostname()
  return `${ts}.${seq}.${pid}.${hostname}:2,`
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
  }>('/messages/send', {
    preHandler: requireAuth,
    schema: {
      description: 'Send an email message',
      tags: ['messages'],
      security: [{ cookieAuth: [] }, { bearerAuth: [] }],
      body: {
        type: 'object',
        required: ['to', 'body'],
        properties: {
          to: { type: 'string', format: 'email', description: 'Recipient email address' },
          subject: { type: 'string', description: 'Message subject' },
          body: { type: 'string', description: 'Message body (plain text)' },
          from: { type: 'string', format: 'email', description: 'Sender override (must be from allowed domain)' },
        },
      },
      response: {
        200: {
          type: 'object',
          properties: {
            id: { type: 'number' },
            status: { type: 'string', enum: ['delivered'] },
            recipient: { type: 'string' },
          },
        },
        202: {
          type: 'object',
          properties: {
            id: { type: 'number' },
            status: { type: 'string', enum: ['queued'] },
            recipient: { type: 'string' },
          },
        },
        400: { type: 'object', properties: { error: { type: 'string' } } },
        401: { type: 'object', properties: { error: { type: 'string' } } },
      },
    },
  }, async (req, reply) => {
    const user = (req as any).user

    const { to, subject, body, from } = req.body || {}
    if (!to || !body) {
      return reply.status(400).send({ error: 'Recipient and body are required' })
    }

    // Validate recipient has @
    if (!to.includes('@')) {
      return reply.status(400).send({ error: 'Invalid recipient format' })
    }

    // Build the .eml message
    const config = db.getServerConfig()
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

    // Save to sent/ folder
    const maildir = process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail'
    const localPart = user.email.split('@')[0]
    const domain = user.email.split('@')[1] || config.domain
    const sentDir = join(maildir, domain, localPart, 'sent')
    await mkdir(sentDir, { recursive: true })
    const sentFilename = generateFilename()
    await writeFile(join(sentDir, sentFilename), eml)

    // Enqueue for delivery
    const recipientDomain = to.split('@')[1]!
    if (db.isDomainAllowed(recipientDomain)) {
      // Local delivery — save directly to recipient's inbox
      const rcptLocal = to.split('@')[0]!
      // For custom domains, find the domain owner's mailbox
      const config2 = db.getServerConfig()
      let rcptMailbox = rcptLocal
      if (recipientDomain !== config2.domain) {
        // Custom domain — route to owner's mailbox
        const ownerEmail = getDb().query(
          'SELECT u.email FROM users u JOIN custom_domains cd ON cd.user_id = u.id WHERE cd.domain = ? AND cd.verified = 1'
        ).get(recipientDomain) as { email: string } | undefined
        if (ownerEmail) {
          rcptMailbox = ownerEmail.email.split('@')[0]!
        }
      }
      const rcptDir = join(maildir, config2.domain, rcptMailbox, 'new')
      await mkdir(rcptDir, { recursive: true })
      await writeFile(join(rcptDir, sentFilename), eml)

      return reply.status(200).send({
        id: 0,
        status: 'delivered',
        recipient: to,
      })
    }

    // External delivery — enqueue
    const outboxDir = join(maildir, domain, localPart, 'outbox')
    await mkdir(outboxDir, { recursive: true })
    const msgFilename = `out-${Date.now()}-${crypto.randomUUID().slice(0, 8)}.eml`
    const msgPath = join(outboxDir, msgFilename)
    await writeFile(msgPath, eml)

    const queueId = db.enqueueMessage(user.id, to, msgPath)

    return reply.status(202).send({
      id: queueId,
      status: 'queued',
      recipient: to,
    })
  })
}
