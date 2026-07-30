#!/usr/bin/env bun
/**
 * Build OpenAPI spec from Fastify routes
 * 
 * Usage: bun run src/scripts/build-docs.ts
 * Output: docs/openapi.json
 */

import Fastify from 'fastify'
import swagger from '@fastify/swagger'
import swaggerUi from '@fastify/swagger-ui'

async function buildSpec() {
  const app = Fastify({ logger: false })

  // Register swagger (without routes)
  await app.register(swagger, {
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

  // Add health endpoint schema manually
  app.get('/health', {
    schema: {
      description: 'Health check',
      tags: ['system'],
      response: {
        200: {
          type: 'object',
          properties: {
            status: { type: 'string' },
            service: { type: 'string' },
          },
        },
      },
    },
  }, async () => {
    return { status: 'ok', service: 'stampd-gateway' }
  })

  // Import and register route modules (just for schema generation)
  // Note: This won't actually connect to DB, just generates schemas
  await import('../routes/auth.js').then(m => m.default(app))
  await import('../routes/mailbox.js').then(m => m.default(app))
  await import('../routes/send.js').then(m => m.default(app))

  // Generate the spec
  const spec = app.swagger()

  // Write to file
  const { mkdir, writeFile } = await import('node:fs/promises')
  const { join } = await import('node:path')
  
  const docsDir = join(process.cwd(), 'docs')
  await mkdir(docsDir, { recursive: true })
  
  const outputPath = join(docsDir, 'openapi.json')
  await writeFile(outputPath, JSON.stringify(spec, null, 2))
  
  console.log(`OpenAPI spec written to ${outputPath}`)
  
  await app.close()
  process.exit(0)
}

buildSpec().catch(err => {
  console.error('Failed to build spec:', err)
  process.exit(1)
})
