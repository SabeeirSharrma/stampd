import { z } from 'zod'

// ── Auth Schemas ──────────────────────────────────────────────────

export const SignupSchema = z.object({
  email: z.string().email('Invalid email format'),
  password: z.string().min(8, 'Password must be at least 8 characters'),
})

export const LoginSchema = z.object({
  email: z.string().email('Invalid email format'),
  password: z.string().min(1, 'Password is required'),
})

export const CreateTokenSchema = z.object({
  label: z.string().min(1, 'Token label is required'),
})

// ── Mailbox Schemas ───────────────────────────────────────────────

export const SendSchema = z.object({
  to: z.string().email('Invalid recipient email'),
  subject: z.string().optional(),
  body: z.string().optional(),
  html: z.string().optional(),
})

export const DraftSchema = z.object({
  to: z.string().email('Invalid recipient email').optional(),
  subject: z.string().optional(),
  body: z.string().optional(),
  html: z.string().optional(),
})

// ── Admin Schemas ─────────────────────────────────────────────────

export const UpdateConfigSchema = z.object({
  domain: z.string().optional(),
  signup_enabled: z.boolean().optional(),
  dkim_selector: z.string().optional(),
})

export const CreateFilterSchema = z.object({
  name: z.string().min(1, 'Filter name is required'),
  path: z.string().min(1, 'Filter path is required'),
  hooks: z.array(z.enum(['mail_from', 'rcpt_to', 'data'])),
  enabled: z.boolean().optional(),
})

export const CustomDomainSchema = z.object({
  domain: z.string().regex(/^[a-z0-9]+([\-\.]{1}[a-z0-9]+)*\.[a-z]{2,}$/, 'Invalid domain format'),
})

// ── Query Schemas ─────────────────────────────────────────────────

export const PaginationSchema = z.object({
  page: z.coerce.number().int().min(1).optional(),
  limit: z.coerce.number().int().min(1).max(100).optional(),
})

export const FilterLogsSchema = PaginationSchema.extend({
  status: z.enum(['accepted', 'rejected', 'bounced']).optional(),
  recipient: z.string().optional(),
  start_date: z.string().optional(),
  end_date: z.string().optional(),
})
