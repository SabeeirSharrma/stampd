import { FastifyInstance } from 'fastify'
import { resolve } from 'node:path'

// Transit imports for cross-language function calls
const __dirname = import.meta.dirname

// Point Transit at the Rust engine and Python admin directories
// Transit scans these directories for exported functions
const { transit } = await import('transit')
const rs = transit.rust(resolve(__dirname, '../../stampd-engine'))
const py = transit.python(resolve(__dirname, '../../stampd-admin'))

// Log discovered Transit functions
transit.info()

async function createApp(): Promise<FastifyInstance> {
  const app = (await import('fastify')).default({
    logger: true,
  })

  // Health check
  app.get('/health', async () => {
    return { status: 'ok', service: 'stampd-gateway' }
  })

  // Engine status via Transit
  app.get('/api/engine/status', async () => {
    try {
      const stats = await rs.getSmtpStats()
      return JSON.parse(stats)
    } catch (err) {
      return { error: 'Engine unavailable' }
    }
  })

  // Queue status via Transit
  app.get('/api/engine/queue', async () => {
    try {
      const status = await rs.getQueueStatus()
      return JSON.parse(status)
    } catch (err) {
      return { error: 'Queue unavailable' }
    }
  })

  // Admin: list users via Transit
  app.get('/api/admin/users', async () => {
    try {
      const users = await py.getUsers({})
      return JSON.parse(users)
    } catch (err) {
      return { error: 'Admin unavailable' }
    }
  })

  // Admin: server config via Transit
  app.get('/api/admin/config', async () => {
    try {
      const config = await py.getServerConfig({})
      return JSON.parse(config)
    } catch (err) {
      return { error: 'Admin unavailable' }
    }
  })

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
