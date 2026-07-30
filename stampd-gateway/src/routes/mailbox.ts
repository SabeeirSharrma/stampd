import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import * as db from '../db.js'
import { readdir, readFile, unlink, stat, writeFile, rename, mkdir } from 'node:fs/promises'
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

  return reply.status(401).send({ error: 'Not authenticated' })
}

async function hashTokenSimple(token: string): Promise<string> {
  const encoder = new TextEncoder()
  const data = encoder.encode(token)
  const hashBuffer = await crypto.subtle.digest('SHA-256', data)
  return Array.from(new Uint8Array(hashBuffer)).map(b => b.toString(16).padStart(2, '0')).join('')
}

// ── Helpers ────────────────────────────────────────────────────

function getMaildir(): string {
  return process.env.STAMPD_MAILDIR || '/var/lib/stampd/mail'
}

function getUserDir(user: any): string {
  const config = db.getServerConfig()
  const localPart = user.email.split('@')[0]
  return join(getMaildir(), config.domain, localPart)
}

function parseHeaders(content: string): Record<string, string> {
  const headers: Record<string, string> = {}
  const lines = content.split('\r\n')
  for (const line of lines) {
    if (line === '') break
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

async function listMaildirFolder(dirPath: string, folder: string) {
  const messages = []
  try {
    const files = await readdir(dirPath)
    for (const file of files) {
      if (file.startsWith('.')) continue
      const filePath = join(dirPath, file)
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
          folder,
        })
      } catch {}
    }
  } catch {}
  return messages
}

function generateFilename(): string {
  const ts = Date.now()
  const seq = Math.floor(Math.random() * 100000)
  const pid = process.pid
  const hostname = require('os').hostname()
  return `${ts}.${seq}.${pid}.${hostname}:2,`
}

// ── Routes ─────────────────────────────────────────────────────

export default async function mailboxRoutes(app: FastifyInstance) {

  // ── GET /mailbox/messages?folder=inbox|sent|archive|spam ──────
  app.get('/mailbox/messages', {
    preHandler: requireAuth,
    schema: {
      description: 'List messages in a folder',
      tags: ['mailbox'],
      security: [{ cookieAuth: [] }, { bearerAuth: [] }],
      querystring: {
        type: 'object',
        properties: {
          folder: { type: 'string', enum: ['inbox', 'sent', 'archive', 'spam', 'drafts'], default: 'inbox' },
        },
      },
      response: {
        200: {
          type: 'object',
          properties: {
            messages: {
              type: 'array',
              items: { $ref: 'MailboxMessage' },
            },
            total: { type: 'number' },
          },
        },
      },
    },
  }, async (req) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const folder = ((req.query as any)?.folder as string) || 'inbox'

    if (folder === 'inbox') {
      const messages = [
        ...(await listMaildirFolder(join(userDir, 'new'), 'new')),
        ...(await listMaildirFolder(join(userDir, 'cur'), 'cur')),
      ]
      messages.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
      return { messages, total: messages.length }
    }

    if (folder === 'drafts') {
      const drafts = await listDrafts(userDir)
      return { messages: drafts, total: drafts.length }
    }

    const messages = await listMaildirFolder(join(userDir, folder), folder)
    messages.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
    return { messages, total: messages.length }
  })

  // ── GET /mailbox/message?id=... (query param, not route param) ──
  app.get('/mailbox/message', {
    preHandler: requireAuth,
    schema: {
      description: 'Get a specific message',
      tags: ['mailbox'],
      security: [{ cookieAuth: [] }, { bearerAuth: [] }],
      querystring: {
        type: 'object',
        required: ['id'],
        properties: {
          id: { type: 'string', description: 'Message filename' },
        },
      },
      response: {
        200: { $ref: 'MailboxMessageDetail' },
        404: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const messageId = (req.query as any)?.id

    if (!messageId) {
      return reply.status(400).send({ error: 'Missing id parameter' })
    }

    // Search across folders
    for (const folder of ['new', 'cur', 'sent', 'archive', 'spam']) {
      const filePath = join(userDir, folder, messageId)
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
          folder,
        }
      } catch {}
    }

    // Check drafts
    try {
      const content = await readFile(join(userDir, 'drafts', messageId), 'utf-8')
      const draft = JSON.parse(content)
      return {
        id: messageId,
        from: user.email,
        to: draft.to || '',
        subject: draft.subject || '(no subject)',
        date: draft.updated_at || draft.created_at,
        size: JSON.stringify(draft).length,
        headers: { from: user.email, to: draft.to, subject: draft.subject },
        body: draft.body || '',
        folder: 'drafts',
        isDraft: true,
      }
    } catch {}

    return reply.status(404).send({ error: 'Message not found' })
  })

  // ── DELETE /mailbox/message — body: {id} ───────────────────────
  app.delete<{
    Body: { id: string }
  }>('/mailbox/message', {
    preHandler: requireAuth,
    schema: {
      description: 'Delete a message',
      tags: ['mailbox'],
      security: [{ cookieAuth: [] }, { bearerAuth: [] }],
      body: {
        type: 'object',
        required: ['id'],
        properties: {
          id: { type: 'string' },
        },
      },
      response: {
        200: { $ref: 'OkResponse' },
        404: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const messageId = (req.body as any)?.id

    if (!messageId) return reply.status(400).send({ error: 'Missing id' })

    for (const folder of ['new', 'cur', 'sent', 'archive', 'spam', 'drafts']) {
      try {
        await unlink(join(userDir, folder, messageId))
        return { ok: true }
      } catch {}
    }

    return reply.status(404).send({ error: 'Message not found' })
  })

  // ── GET /mailbox/stats ─────────────────────────────────────────
  app.get('/mailbox/stats', {
    preHandler: requireAuth,
    schema: {
      description: 'Get mailbox statistics',
      tags: ['mailbox'],
      security: [{ cookieAuth: [] }, { bearerAuth: [] }],
      response: {
        200: {
          type: 'object',
          properties: {
            unread: { type: 'number' },
            total: { type: 'number' },
            size_bytes: { type: 'number' },
            quota_mb: { type: 'number' },
          },
        },
      },
    },
  }, async (req) => {
    const user = (req as any).user
    const userDir = getUserDir(user)

    let unread = 0
    let total = 0
    let totalSize = 0

    for (const subdir of ['new', 'cur', 'sent', 'archive', 'spam']) {
      try {
        const files = await readdir(join(userDir, subdir))
        const validFiles = files.filter(f => !f.startsWith('.'))
        total += validFiles.length
        if (subdir === 'new') unread = validFiles.length
        for (const f of validFiles) {
          try {
            const s = await stat(join(userDir, subdir, f))
            totalSize += s.size
          } catch {}
        }
      } catch {}
    }

    return { unread, total, size_bytes: totalSize, quota_mb: 5120 }
  })

  // ── POST /mailbox/action — body: {action, id} ──────────────────
  // Handles: archive, unarchive, spam, not-spam, mark-read
  app.post<{
    Body: { action: string; id: string }
  }>('/mailbox/action', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const { action, id: messageId } = (req.body as any) || {}

    if (!action || !messageId) {
      return reply.status(400).send({ error: 'Missing action or id' })
    }

    switch (action) {
      case 'archive': {
        for (const folder of ['new', 'cur']) {
          try {
            const src = join(userDir, folder, messageId)
            await stat(src)
            await rename(src, join(userDir, 'archive', messageId))
            return { ok: true, folder: 'archive' }
          } catch {}
        }
        return reply.status(404).send({ error: 'Message not found in inbox' })
      }

      case 'unarchive': {
        try {
          const src = join(userDir, 'archive', messageId)
          await stat(src)
          await rename(src, join(userDir, 'cur', messageId))
          return { ok: true, folder: 'cur' }
        } catch {}
        return reply.status(404).send({ error: 'Message not found in archive' })
      }

      case 'spam': {
        for (const folder of ['new', 'cur']) {
          try {
            const src = join(userDir, folder, messageId)
            await stat(src)
            await rename(src, join(userDir, 'spam', messageId))
            return { ok: true, folder: 'spam' }
          } catch {}
        }
        return reply.status(404).send({ error: 'Message not found in inbox' })
      }

      case 'not-spam': {
        try {
          const src = join(userDir, 'spam', messageId)
          await stat(src)
          await rename(src, join(userDir, 'cur', messageId))
          return { ok: true, folder: 'cur' }
        } catch {}
        return reply.status(404).send({ error: 'Message not found in spam' })
      }

      case 'mark-read': {
        try {
          const src = join(userDir, 'new', messageId)
          await stat(src)
          await rename(src, join(userDir, 'cur', messageId))
          return { ok: true, folder: 'cur' }
        } catch {}
        return reply.status(404).send({ error: 'Message not found or already read' })
      }

      default:
        return reply.status(400).send({ error: `Unknown action: ${action}` })
    }
  })

  // ── DRAFTS ────────────────────────────────────────────────────

  // GET /mailbox/drafts
  app.get('/mailbox/drafts', { preHandler: requireAuth }, async (req) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const drafts = await listDrafts(userDir)
    return { messages: drafts, total: drafts.length }
  })

  // POST /mailbox/drafts — save new draft
  app.post<{
    Body: { to?: string; subject?: string; body?: string; draft_id?: string }
  }>('/mailbox/drafts', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const { to, subject, body, draft_id } = (req.body as any) || {}

    const now = new Date().toISOString()
    const draftData = {
      to: to || '',
      subject: subject || '',
      body: body || '',
      created_at: now,
      updated_at: now,
    }

    const draftsDir = join(userDir, 'drafts')
    await mkdir(draftsDir, { recursive: true })

    let filename = draft_id
    if (!filename) {
      filename = `draft-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.json`
    }

    const filePath = join(draftsDir, filename)

    // If updating existing draft, preserve created_at
    try {
      const existing = await readFile(filePath, 'utf-8')
      const parsed = JSON.parse(existing)
      draftData.created_at = parsed.created_at || now
    } catch {}

    await writeFile(filePath, JSON.stringify(draftData, null, 2))
    return reply.status(201).send({ ok: true, id: filename, draft: draftData })
  })

  // PUT /mailbox/draft — update draft, body: {id, to?, subject?, body?}
  app.put<{
    Body: { id: string; to?: string; subject?: string; body?: string }
  }>('/mailbox/draft', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const { id: draftId, to, subject, body } = (req.body as any) || {}

    if (!draftId) return reply.status(400).send({ error: 'Missing draft id' })

    const filePath = join(userDir, 'drafts', draftId)

    let existing: any = {}
    try {
      const content = await readFile(filePath, 'utf-8')
      existing = JSON.parse(content)
    } catch {
      return reply.status(404).send({ error: 'Draft not found' })
    }

    const draftData = {
      to: to ?? existing.to ?? '',
      subject: subject ?? existing.subject ?? '',
      body: body ?? existing.body ?? '',
      created_at: existing.created_at,
      updated_at: new Date().toISOString(),
    }

    await writeFile(filePath, JSON.stringify(draftData, null, 2))
    return { ok: true, id: draftId, draft: draftData }
  })

  // DELETE /mailbox/draft — body: {id}
  app.delete<{
    Body: { id: string }
  }>('/mailbox/draft', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const draftId = (req.body as any)?.id

    if (!draftId) return reply.status(400).send({ error: 'Missing draft id' })

    try {
      await unlink(join(userDir, 'drafts', draftId))
      return { ok: true }
    } catch {
      return reply.status(404).send({ error: 'Draft not found' })
    }
  })

  // POST /mailbox/send-draft — body: {id}
  app.post<{
    Body: { id: string }
  }>('/mailbox/send-draft', { preHandler: requireAuth }, async (req, reply) => {
    const user = (req as any).user
    const userDir = getUserDir(user)
    const draftId = (req.body as any)?.id

    if (!draftId) return reply.status(400).send({ error: 'Missing draft id' })

    const filePath = join(userDir, 'drafts', draftId)
    let draft: any
    try {
      const content = await readFile(filePath, 'utf-8')
      draft = JSON.parse(content)
    } catch {
      return reply.status(404).send({ error: 'Draft not found' })
    }

    if (!draft.to) {
      return reply.status(400).send({ error: 'Draft has no recipient' })
    }

    // Build the .eml
    const config = db.getServerConfig()
    const emlId = `<${crypto.randomUUID()}@${config.domain}>`
    const now = new Date().toUTCString()

    const eml = [
      `From: ${user.email}`,
      `To: ${draft.to}`,
      `Subject: ${draft.subject || '(no subject)'}`,
      `Date: ${now}`,
      `Message-ID: ${emlId}`,
      `MIME-Version: 1.0`,
      `Content-Type: text/plain; charset=utf-8`,
      '',
      draft.body || '',
    ].join('\r\n')

    // Save to sent/
    const sentDir = join(userDir, 'sent')
    await mkdir(sentDir, { recursive: true })
    await writeFile(join(sentDir, generateFilename()), eml)

    // Enqueue for delivery
    const recipientDomain = draft.to.split('@')[1]
    if (recipientDomain === config.domain) {
      // Local delivery
      const rcptLocal = draft.to.split('@')[0]
      const rcptDir = join(getMaildir(), config.domain, rcptLocal, 'new')
      await mkdir(rcptDir, { recursive: true })
      await writeFile(join(rcptDir, generateFilename()), eml)
    } else {
      // External — enqueue
      const localPart = user.email.split('@')[0]
      const domain = user.email.split('@')[1] || config.domain
      const outboxDir = join(getMaildir(), domain, localPart, 'outbox')
      await mkdir(outboxDir, { recursive: true })
      const outPath = join(outboxDir, `out-${Date.now()}.eml`)
      await writeFile(outPath, eml)
      db.enqueueMessage(user.id, draft.to, outPath)
    }

    // Delete the draft
    await unlink(filePath)

    return reply.status(202).send({ ok: true, status: 'queued', recipient: draft.to })
  })
}

// ── Draft listing helper ───────────────────────────────────────

async function listDrafts(userDir: string) {
  const draftsDir = join(userDir, 'drafts')
  const messages: any[] = []
  try {
    const files = await readdir(draftsDir)
    for (const file of files) {
      if (!file.endsWith('.json')) continue
      const filePath = join(draftsDir, file)
      try {
        const content = await readFile(filePath, 'utf-8')
        const draft = JSON.parse(content)
        const fileInfo = await stat(filePath)
        messages.push({
          id: file,
          from: '',
          to: draft.to || '',
          subject: draft.subject || '(no subject)',
          date: draft.updated_at || draft.created_at || fileInfo.mtime.toISOString(),
          size: fileInfo.size,
          path: filePath,
          folder: 'drafts',
          isDraft: true,
          body: draft.body || '',
        })
      } catch {}
    }
  } catch {}
  messages.sort((a, b) => new Date(b.date).getTime() - new Date(a.date).getTime())
  return messages
}
