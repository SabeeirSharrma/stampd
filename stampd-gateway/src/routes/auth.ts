import type { FastifyInstance } from 'fastify'
import * as db from '../db.js'
import { hashToken } from '../auth.js'
import { hash } from 'argon2'
import { SignupSchema, LoginSchema, CreateTokenSchema } from '../schemas.js'

const COOKIE_SECURE = process.env.COOKIE_SECURE !== 'false'
const COOKIE_SAMESITE = (process.env.COOKIE_SAMESITE as any) || 'lax'

export default async function authRoutes(app: FastifyInstance) {
  // ── POST /auth/signup ──────────────────────────────────────────
  app.post<{
    Body: { email: string; password: string }
  }>('/auth/signup', {
    schema: {
      description: 'Create a new user account',
      tags: ['auth'],
      body: {
        type: 'object',
        required: ['email', 'password'],
        properties: {
          email: { type: 'string', format: 'email' },
          password: { type: 'string', minLength: 8 },
        },
      },
      response: {
        201: {
          type: 'object',
          properties: {
            id: { type: 'number' },
            email: { type: 'string' },
            is_admin: { type: 'boolean' },
          },
        },
        403: { $ref: 'Error' },
        409: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const config = db.getServerConfig()
    if (!config.signup_enabled) {
      return reply.status(403).send({ error: 'Self-signup is disabled' })
    }

    const parsed = SignupSchema.safeParse(req.body)
    if (!parsed.success) {
      return reply.status(400).send({ error: parsed.error.issues[0].message })
    }

    const { email, password } = parsed.data

    // Check if user already exists
    const existing = db.getUserByEmail(email)
    if (existing) {
      return reply.status(409).send({ error: 'Email already registered' })
    }

    // Validate domain matches our server
    const domain = email.split('@')[1]
    if (domain !== config.domain) {
      return reply.status(400).send({ error: `Only @${config.domain} addresses are allowed` })
    }

    // Hash password with argon2id
    const passwordHash = await hash(password)

    // Check if this is the first user (make them admin)
    const users = db.listUsers()
    const isFirstUser = users.length === 0

    const userId = db.createUser(email, passwordHash, isFirstUser)

    // Create session
    const expiresAt = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60 // 30 days
    const sessionId = db.createSession(userId, expiresAt)

    // Set session cookie
    reply.setCookie('stampd-session', sessionId, {
      path: '/',
      httpOnly: true,
      secure: COOKIE_SECURE,
      sameSite: COOKIE_SAMESITE,
      maxAge: 30 * 24 * 60 * 60, // 30 days
    })

    return reply.status(201).send({
      id: userId,
      email,
      is_admin: isFirstUser,
    })
  })

  // ── POST /auth/login ──────────────────────────────────────────
  app.post<{
    Body: { email: string; password: string }
  }>('/auth/login', {
    schema: {
      description: 'Authenticate with email and password',
      tags: ['auth'],
      body: {
        type: 'object',
        required: ['email', 'password'],
        properties: {
          email: { type: 'string', format: 'email' },
          password: { type: 'string', minLength: 1 },
        },
      },
      response: {
        200: {
          type: 'object',
          properties: {
            id: { type: 'number' },
            email: { type: 'string' },
            is_admin: { type: 'boolean' },
          },
        },
        401: { $ref: 'Error' },
        403: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const parsed = LoginSchema.safeParse(req.body)
    if (!parsed.success) {
      return reply.status(400).send({ error: parsed.error.issues[0].message })
    }

    const { email, password } = parsed.data

    const user = db.getUserByEmail(email)
    if (!user) {
      return reply.status(401).send({ error: 'Invalid credentials' })
    }

    if (user.disabled_at) {
      return reply.status(403).send({ error: 'Account disabled' })
    }

    // Verify password with argon2id
    const { verify } = await import('argon2')
    const valid = await verify(user.password_hash, password)
    if (!valid) {
      return reply.status(401).send({ error: 'Invalid credentials' })
    }

    // Create session
    const expiresAt = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60
    const sessionId = db.createSession(user.id, expiresAt)

    reply.setCookie('stampd-session', sessionId, {
      path: '/',
      httpOnly: true,
      secure: COOKIE_SECURE,
      sameSite: COOKIE_SAMESITE,
      maxAge: 30 * 24 * 60 * 60,
    })

    return reply.send({
      id: user.id,
      email: user.email,
      is_admin: !!user.is_admin,
    })
  })

  // ── POST /auth/logout ─────────────────────────────────────────
  app.post('/auth/logout', {
    schema: {
      description: 'End current session',
      tags: ['auth'],
      response: {
        200: {
          type: 'object',
          properties: { ok: { type: 'boolean' } },
        },
      },
    },
  }, async (req, reply) => {
    const sessionId = req.cookies?.['stampd-session']
    if (sessionId) {
      db.deleteSession(sessionId)
    }
    reply.clearCookie('stampd-session', { path: '/' })
    return { ok: true }
  })

  // ── POST /auth/tokens ─────────────────────────────────────────
  app.post<{
    Body: { label: string }
  }>('/auth/tokens', {
    preHandler: requireAuthForTokens,
    schema: {
      description: 'Create a new API token',
      tags: ['auth'],
      security: [{ cookieAuth: [] }],
      body: {
        type: 'object',
        required: ['label'],
        properties: {
          label: { type: 'string', minLength: 1 },
        },
      },
      response: {
        201: {
          type: 'object',
          properties: {
            id: { type: 'number' },
            label: { type: 'string' },
            scope: { type: 'string' },
            token: { type: 'string', description: 'Token value (shown only once)' },
          },
        },
        401: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const user = (req as any).user

    const parsed = CreateTokenSchema.safeParse(req.body)
    if (!parsed.success) {
      return reply.status(400).send({ error: parsed.error.issues[0].message })
    }

    const { label } = parsed.data

    // Generate random token
    const rawToken = crypto.randomUUID().replace(/-/g, '')
    const tokenHash = await hashToken(rawToken)

    const tokenId = db.createToken(user.id, tokenHash, label)

    // Return the raw token ONLY on creation
    return reply.status(201).send({
      id: tokenId,
      label,
      scope: 'send',
      token: rawToken, // Only shown once!
    })
  })

  // ── GET /auth/tokens ──────────────────────────────────────────
  app.get('/auth/tokens', {
    preHandler: requireAuthForTokens,
    schema: {
      description: 'List all API tokens',
      tags: ['auth'],
      security: [{ cookieAuth: [] }],
      response: {
        200: {
          type: 'array',
          items: {
            type: 'object',
            properties: {
              id: { type: 'number' },
              label: { type: 'string' },
              scope: { type: 'string' },
              created_at: { type: 'string' },
              last_used_at: { type: 'string', nullable: true },
            },
          },
        },
        401: { $ref: 'Error' },
      },
    },
  }, async (req) => {
    const user = (req as any).user
    return db.listUserTokens(user.id)
  })

  // ── DELETE /auth/tokens/:id ────────────────────────────────────
  app.delete<{ Params: { id: string } }>('/auth/tokens/:id', {
    preHandler: requireAuthForTokens,
    schema: {
      description: 'Revoke an API token',
      tags: ['auth'],
      security: [{ cookieAuth: [] }],
      params: {
        type: 'object',
        required: ['id'],
        properties: {
          id: { type: 'string' },
        },
      },
      response: {
        200: {
          type: 'object',
          properties: { ok: { type: 'boolean' } },
        },
        401: { $ref: 'Error' },
        404: { $ref: 'Error' },
      },
    },
  }, async (req, reply) => {
    const user = (req as any).user
    const tokenId = parseInt(req.params.id)
    if (isNaN(tokenId)) {
      return reply.status(400).send({ error: 'Invalid token id' })
    }

    // Verify the token belongs to this user
    const tokens = db.listUserTokens(user.id) as any[]
    const token = tokens.find((t: any) => t.id === tokenId)
    if (!token) {
      return reply.status(404).send({ error: 'Token not found' })
    }

    db.revokeToken(tokenId)
    return { ok: true }
  })
}

// ── Auth helper for token routes ────────────────────────────────
async function requireAuthForTokens(req: any, reply: any) {
  const sessionId = req.cookies?.['stampd-session']
  if (sessionId) {
    const userId = db.validateSession(sessionId)
    if (userId) {
      const user = db.getUserById(userId)
      if (user && !user.disabled_at) {
        const { id: _dbId, ...userData } = user
        req.user = { id: userId, ...userData }
        return
      }
    }
  }
  return reply.status(401).send({ error: 'Authentication required' })
}
