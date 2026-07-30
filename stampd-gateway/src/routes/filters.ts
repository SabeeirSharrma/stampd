/**
 * Filter routes — engine delegates filter execution here via Transit.
 *
 * POST /internal/filters/run — run a single filter function
 * POST /internal/filters/hook  — run all filters for a hook point
 */

import type { FastifyInstance, FastifyRequest, FastifyReply } from 'fastify'
import { runFilter, runFiltersForHook, initFilters } from '../filters.js'

// Initialize Transit Python bridge on module load
let filtersReady = false

async function ensureFilters() {
  if (!filtersReady) {
    filtersReady = initFilters()
  }
  return filtersReady
}

export default async function filterRoutes(app: FastifyInstance) {
  // Ensure filters are initialized
  await ensureFilters()

  // ── POST /internal/filters/run ──────────────────────────────
  app.post<{
    Body: {
      function: string
      context: Record<string, unknown>
    }
  }>('/internal/filters/run', async (req, reply) => {
    const { function: fnName, context } = (req.body as any) || {}
    if (!fnName || !context) {
      return reply.status(400).send({ error: 'Missing function or context' })
    }

    const result = await runFilter(fnName, context)
    return result
  })

  // ── POST /internal/filters/hook ─────────────────────────────
  app.post<{
    Body: {
      hook: string
      context: Record<string, unknown>
      filters: string[]
    }
  }>('/internal/filters/hook', async (req, reply) => {
    const { hook, context, filters } = (req.body as any) || {}
    if (!hook || !context || !Array.isArray(filters)) {
      return reply.status(400).send({ error: 'Missing hook, context, or filters' })
    }

    const result = await runFiltersForHook(hook, context, filters)
    return result
  })
}
