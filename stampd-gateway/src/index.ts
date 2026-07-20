import { FastifyInstance } from 'fastify'

async function createApp(): Promise<FastifyInstance> {
  const app = (await import('fastify')).default({
    logger: true,
  })

  // Health check
  app.get('/health', async () => {
    return { status: 'ok', service: 'stampd-gateway' }
  })

  // TODO: Add auth middleware
  // TODO: Add rate limiting
  // TODO: Add CORS configuration
  // TODO: Add API endpoints per OpenAPI spec

  return app
}

const start = async () => {
  const app = await createApp()
  const port = parseInt(process.env.GATEWAY_PORT || '8080')

  try {
    await app.listen({ port, host: '0.0.0.0' })
    console.log(`stampd-gateway listening on port ${port}`)
  } catch (err) {
    app.log.error(err)
    process.exit(1)
  }
}

start()
