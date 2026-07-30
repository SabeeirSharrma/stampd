import type { FastifyInstance } from 'fastify'
import authRoutes from './routes/auth.js'
import mailboxRoutes from './routes/mailbox.js'
import sendRoutes from './routes/send.js'
import adminRoutes from './routes/admin.js'
import filterRoutes from './routes/filters.js'
import { stopFilters } from './filters.js'

async function createApp(): Promise<FastifyInstance> {
  const app = (await import('fastify')).default({
    logger: true,
  })

  // ── Plugins ────────────────────────────────────────────────────
  // Allow all origins in dev; lock down in production
  // origin: true reflects the request origin — works with credentials
  const allowedOrigins = process.env.CORS_ORIGINS?.split(',') || ['*']

  await app.register((await import('@fastify/cors')).default, {
    origin: allowedOrigins.includes('*') ? true : allowedOrigins,
    credentials: true,
  })

  await app.register((await import('@fastify/cookie')).default)

  await app.register((await import('@fastify/rate-limit')).default, {
    max: parseInt(process.env.RATE_LIMIT || '60'),
    timeWindow: '1 minute',
  })

  // ── Swagger/OpenAPI ────────────────────────────────────────────
  await app.register((await import('@fastify/swagger')).default, {
    openapi: {
      info: {
        title: 'Stampd API',
        description: 'Self-hosted mail server API',
        version: '0.7.0',
      },
      servers: [
        { url: 'http://localhost:8080', description: 'Development' },
      ],
      components: {
        securitySchemes: {
          cookieAuth: {
            type: 'apiKey',
            in: 'cookie',
            name: 'stampd-session',
            description: 'Session cookie from web UI login',
          },
          bearerAuth: {
            type: 'http',
            scheme: 'bearer',
            description: 'API token for programmatic access',
          },
        },
        schemas: {
          Error: {
            type: 'object',
            properties: {
              error: { type: 'string' },
            },
          },
          OkResponse: {
            type: 'object',
            properties: {
              ok: { type: 'boolean' },
            },
          },
          MailboxMessage: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              from: { type: 'string' },
              to: { type: 'string' },
              subject: { type: 'string' },
              date: { type: 'string', format: 'date-time' },
              size: { type: 'number' },
              folder: { type: 'string' },
            },
          },
          MailboxMessageDetail: {
            type: 'object',
            properties: {
              id: { type: 'string' },
              from: { type: 'string' },
              to: { type: 'string' },
              subject: { type: 'string' },
              date: { type: 'string', format: 'date-time' },
              size: { type: 'number' },
              headers: { type: 'object', additionalProperties: { type: 'string' } },
              body: { type: 'string' },
              folder: { type: 'string' },
              isDraft: { type: 'boolean' },
            },
          },
        },
      },
      security: [
        { cookieAuth: [] },
        { bearerAuth: [] },
      ],
    },
  })

  await app.register((await import('@fastify/swagger-ui')).default, {
    routePrefix: '/docs',
    uiConfig: {
      docExpansion: 'list',
      deepLinking: true,
    },
  })

  // ── Health ─────────────────────────────────────────────────────
  app.get('/health', async () => {
    return { status: 'ok', service: 'stampd-gateway' }
  })

  // ── Routes ─────────────────────────────────────────────────────
  await app.register(authRoutes)
  await app.register(mailboxRoutes)
  await app.register(sendRoutes)
  await app.register(adminRoutes)
  await app.register(filterRoutes)

  // ── Graceful Shutdown ──────────────────────────────────────────
  app.addHook('onClose', async () => {
    await stopFilters()
  })

  return app
}

const start = async () => {
  const app = await createApp()
  const port = parseInt(process.env.GATEWAY_PORT || '8080')

  try {
    await app.listen({ port, host: '0.0.0.0' })
    console.log(`stampd-gateway listening on port ${port}`)
    console.log(`API docs available at http://localhost:${port}/docs`)
  } catch (err) {
    app.log.error(err)
    process.exit(1)
  }
}

start()
