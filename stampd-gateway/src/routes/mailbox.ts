import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import * as db from '../db.js'
import { readdir, readFile, unlink, stat } from 'node:fs/promises'
import { join } from 'node:path'

async function requireAuth(req: FastifyRequest, reply: FastifyReply) {
  // Try session cookie
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

  // Try Bearer token
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

  return reply.status(401).send({ error: 'Not authenticated' })
}

async function hashTokenSimple(token: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(token)
  const hashBuffer = await crypto.subtle.digest('SHA-256', data)
  return Array.from(new Uint8Array(hashBuffer)).map(b => b.toString(16).padStart(2, '0')).join('')
}

export default async function mailboxRoutes(app: FastifyInstance) {
  // ── GET /mailbox/messages ──────────────────────────────────────
  app.get('/mailbox/messages', { preHandler: requireAuth }, async (req) => {

    const config = db.getServerConfig()
    const domain = config.domain
    const localPart = user.email.split('@')[0]

    const newDir = join(process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail', domain, localPart, 'new')
    const curDir = join(process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail', domain, localPart, 'cur')

    const messages = []

    // Read from new/ directory
    try {
      const files = await readdir(newDir)
      for (const file of files) {
        const filePath = join(newDir, file)
        try {
          const content = await readFile(filePath, 'utf-8')
          const headers = parseHeaders(content)
          const fileInfo = await stat(filePath)
          messages.push({
            id: file,
            from: headers.from || '',
            to: headers.to || '',
            subject: headers.subject || '(no subject)',
            date: headers.date || fileInfo.mtime.toISOString(),
            size: fileInfo.size,
            path: filePath,
            folder: 'new',
          })
        } catch {
          // Skip unreadable files
        }
      }
    } catch {
      // new/ directory may not exist yet
    }

    // Read from cur/ directory
    try {
      const files = await readdir(curDir)
      for (const file of files) {
        const filePath = join(curDir, file)
        try {
          const content = await readFile(filePath, 'utf-8')
          const headers = parseHeaders(content)
          const fileInfo = await stat(filePath)
          messages.push({
            id: file,
            from: headers.from || '',
            to: headers.to || '',
            subject: headers.subject || '(no subject)',
            date: headers.date || fileInfo.mtime.toISOString(),
            size: fileInfo.size,
            path: filePath,
            folder: 'cur',
          })
        } catch {
          // Skip unreadable files
        }
      }
    } catch {
      // cur/ directory may not exist yet
    }

    // Sort by date descending
    messages.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())

    return { messages, total: messages.length }
  })

  // ── GET /mailbox/messages/:id ──────────────────────────────────
  app.get<{ Params: { id: string } }>('/mailbox/messages/:id', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const config = db.getServerConfig()
    const localPart = user.email.split('@')[0]
    const messageId = req.params.id
    const maildir = process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail'

    // Search in both new/ and cur/
    const dirs = ['new', 'cur']
    for (const dir of dirs) {
      const filePath = join(maildir, config.domain, localPart, dir, messageId)
      try {
        const content = await readFile(filePath, 'utf-8')
        const headers = parseHeaders(content)
        const body = extractBody(content)
        const fileInfo = await stat(filePath)

        return {
          id: messageId,
          from: headers.from || '',
          to: headers.to || '',
          subject: headers.subject || '(no subject)',
          date: headers.date || fileInfo.mtime.toISOString(),
          size: fileInfo.size,
          headers,
          body,
        }
      } catch {
        // File not found in this directory, try next
      }
    }

    return reply.status(404).send({ error: 'Message not found' })
  })

  // ── DELETE /mailbox/messages/:id ───────────────────────────────
  app.delete<{ Params: { id: string } }>('/mailbox/messages/:id', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const config = db.getServerConfig()
    const localPart = user.email.split('@')[0]
    const messageId = req.params.id
    const maildir = process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail'

    const dirs = ['new', 'cur']
    for (const dir of dirs) {
      const filePath = join(maildir, config.domain, localPart, dir, messageId)
      try {
        await unlink(filePath)
        return { ok: true }
      } catch {
        // Try next directory
      }
    }

    return reply.status(404).send({ error: 'Message not found' })
  })

  // ── GET /mailbox/stats ─────────────────────────────────────────
  app.get('/mailbox/stats', { preHandler: requireAuth }, async (req) => {
    const user = (req as any).user
    const config = db.getServerConfig()
    const localPart = user.email.split('@')[0]
    const mailboxPath = join(process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail', config.domain, localPart)

    let newCount = 0
    let curCount = 0
    let totalSize = 0

    try {
      const newFiles = await readdir(join(mailboxPath, 'new'))
      newCount = newFiles.length
      for (const f of newFiles) {
        const s = await stat(join(mailboxPath, 'new', f))
        totalSize += s.size
      }
    } catch {}

    try {
      const curFiles = await readdir(join(mailboxPath, 'cur'))
      curCount = curFiles.length
      for (const f of curFiles) {
        const s = await stat(join(mailboxPath, 'cur', f))
        totalSize += s.size
      }
    } catch {}

    const quotaMb = 5120 // TODO: read from config
    return {
      unread: newCount,
      total: newCount + curCount,
      size_bytes: totalSize,
      quota_mb: quotaMb,
    }
  })
}

// ── Helpers ────────────────────────────────────────────────────

function parseHeaders(content: string): Record<string, string> {
  const headers: Record<string, string> = {}
  const lines = content.split('\r\n')

  for (const line of lines) {
    if (line === '') break // End of headers
    const colonIndex = line.indexOf(':')
    if (colonIndex > 0) {
      const key = line.slice(0, colonIndex).toLowerCase().trim()
      const value = line.slice(colonIndex + 1).trim()
      headers[key] = value
    }
  }

  return headers
}

function extractBody(content: string): string {
  const parts = content.split('\r\n\r\n')
  return parts.slice(1).join('\r\n\r\n')
}
