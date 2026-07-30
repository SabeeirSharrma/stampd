/**
 * Stampd Gateway API client.
 * All requests use credentials: 'include' for session cookies.
 */

const BASE = import.meta.env.PUBLIC_API_URL ?? "";

async function request<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const opts: RequestInit = {
    method,
    credentials: "include",
    headers: { "Content-Type": "application/json" },
  };
  if (body !== undefined) opts.body = JSON.stringify(body);

  const res = await fetch(`${BASE}${path}`, opts);
  const data = await res.json();

  if (!res.ok) {
    const msg = (data as any)?.error ?? `Request failed (${res.status})`;
    throw new ApiError(res.status, msg);
  }
  return data as T;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

// ── Auth ──────────────────────────────────────────────────────

export interface User {
  id: number;
  email: string;
  is_admin: boolean;
}

export async function signup(email: string, password: string): Promise<User> {
  return request("POST", "/auth/signup", { email, password });
}

export async function login(email: string, password: string): Promise<User> {
  return request("POST", "/auth/login", { email, password });
}

export async function logout(): Promise<void> {
  await request("POST", "/auth/logout");
}

// ── Mailbox ───────────────────────────────────────────────────

export interface MailboxMessage {
  id: string;
  from: string;
  to: string;
  subject: string;
  date: string;
  size: number;
  path: string;
  folder: "new" | "cur";
  isDraft?: boolean;
  body?: string;
}

export interface MessageDetail extends MailboxMessage {
  headers: Record<string, string>;
  body: string;
}

export interface MailboxStats {
  unread: number;
  total: number;
  size_bytes: number;
  quota_mb: number;
}

export async function listMessages(
  folder?: "inbox" | "sent" | "archive" | "spam" | "drafts",
): Promise<{
  messages: MailboxMessage[];
  total: number;
}> {
  const qs = folder ? `?folder=${encodeURIComponent(folder)}` : "";
  return request("GET", `/mailbox/messages${qs}`);
}

export async function getMessage(id: string): Promise<MessageDetail> {
  return request("GET", `/mailbox/message?id=${encodeURIComponent(id)}`);
}

export async function deleteMessage(id: string): Promise<void> {
  await request("DELETE", "/mailbox/message", { id });
}

export async function getMailboxStats(): Promise<MailboxStats> {
  return request("GET", "/mailbox/stats");
}

// ── Drafts ────────────────────────────────────────────────────

export interface Draft {
  id: string;
  to?: string;
  subject?: string;
  body?: string;
  draft_id?: string;
  created_at: string;
  updated_at: string;
}

export async function listDrafts(): Promise<{ drafts: Draft[] }> {
  return request("GET", "/mailbox/drafts");
}

export async function saveDraft(draft: {
  to?: string;
  subject?: string;
  body?: string;
  draft_id?: string;
}): Promise<Draft> {
  return request("POST", "/mailbox/drafts", draft);
}

export async function updateDraft(
  id: string,
  draft: { to?: string; subject?: string; body?: string },
): Promise<Draft> {
  return request("PUT", "/mailbox/draft", { id, ...draft });
}

export async function deleteDraft(id: string): Promise<void> {
  await request("DELETE", "/mailbox/draft", { id });
}

export async function sendDraft(id: string): Promise<void> {
  await request("POST", "/mailbox/send-draft", { id });
}

// ── Message Actions ───────────────────────────────────────────

export async function archiveMessage(id: string): Promise<void> {
  await request("POST", "/mailbox/action", { action: "archive", id });
}

export async function unarchiveMessage(id: string): Promise<void> {
  await request("POST", "/mailbox/action", { action: "unarchive", id });
}

export async function reportSpam(id: string): Promise<void> {
  await request("POST", "/mailbox/action", { action: "spam", id });
}

export async function notSpam(id: string): Promise<void> {
  await request("POST", "/mailbox/action", { action: "not-spam", id });
}

export async function markRead(id: string): Promise<void> {
  await request("POST", "/mailbox/action", { action: "mark-read", id });
}

// ── Send ──────────────────────────────────────────────────────

export interface SendResult {
  id: number;
  status: string;
  recipient: string;
}

export async function sendMessage(
  to: string,
  subject: string,
  body: string,
  from?: string,
): Promise<SendResult> {
  return request("POST", "/messages/send", { to, subject, body, from });
}

// ── Tokens ────────────────────────────────────────────────────

export interface Token {
  id: number;
  label: string;
  scope: string;
  created_at: number;
  last_used_at: number | null;
  revoked: boolean;
}

export async function createToken(label: string): Promise<Token & { token: string }> {
  return request("POST", "/auth/tokens", { label });
}

export async function listTokens(): Promise<Token[]> {
  return request("GET", "/auth/tokens");
}

export async function revokeToken(id: number): Promise<void> {
  await request("DELETE", `/auth/tokens/${id}`);
}

// ── Admin ─────────────────────────────────────────────────────

export interface AdminUser {
  id: number;
  email: string;
  is_admin: number;
  disabled: boolean;
}

export interface ServerConfig {
  domain: string;
  signup_enabled: number;
  dkim_selector: string;
}

export interface QueueMessage {
  id: number;
  from_user_id: number;
  recipient: string;
  message_path: string;
  attempts: number;
  next_attempt_at: number;
  last_error: string | null;
  status: string;
}

export interface DeliveryLog {
  id: number;
  queue_id: number;
  status: string;
  recipient: string;
  error: string | null;
  created_at: number;
}

export interface FilterRecord {
  id: number;
  name: string;
  path: string;
  hooks: string;
  enabled: boolean;
  created_at: number;
  updated_at: number;
}

export interface QuotaEntry {
  id: number;
  email: string;
  is_admin: number;
  disabled: boolean;
  size_bytes: number;
  message_count: number;
}

export async function adminListUsers(): Promise<AdminUser[]> {
  return request("GET", "/admin/users");
}

export async function adminDisableUser(id: number): Promise<void> {
  await request("PATCH", `/admin/users/${id}/disable`);
}

export async function adminDeleteUser(id: number): Promise<void> {
  await request("DELETE", `/admin/users/${id}`);
}

export async function adminGetConfig(): Promise<ServerConfig> {
  return request("GET", "/admin/config");
}

export async function adminUpdateConfig(
  updates: Partial<Pick<ServerConfig, "domain" | "signup_enabled" | "dkim_selector">>,
): Promise<{ ok: boolean; config: ServerConfig }> {
  return request("PATCH", "/admin/config", updates);
}

export async function adminListQueue(
  status?: string,
): Promise<QueueMessage[]> {
  const qs = status ? `?status=${encodeURIComponent(status)}` : "";
  return request("GET", `/admin/queue${qs}`);
}

export async function adminRetryMessage(id: number): Promise<void> {
  await request("POST", `/admin/queue/${id}/retry`);
}

export async function adminPurgeMessage(id: number): Promise<void> {
  await request("DELETE", `/admin/queue/${id}`);
}

export async function adminListLogs(
  filters?: { status?: string; recipient?: string; limit?: number },
): Promise<DeliveryLog[]> {
  const params = new URLSearchParams();
  if (filters?.status) params.set("status", filters.status);
  if (filters?.recipient) params.set("recipient", filters.recipient);
  if (filters?.limit) params.set("limit", String(filters.limit));
  const qs = params.toString();
  return request("GET", `/admin/logs${qs ? `?${qs}` : ""}`);
}

export async function adminListFilters(): Promise<FilterRecord[]> {
  return request("GET", "/admin/filters");
}

export async function adminCreateFilter(
  name: string,
  path: string,
  hooks: string[],
): Promise<{ ok: boolean; id: number; filter: FilterRecord }> {
  return request("POST", "/admin/filters", { name, path, hooks });
}

export async function adminToggleFilter(
  id: number,
  enabled: boolean,
): Promise<void> {
  await request("PATCH", `/admin/filters/${id}`, { enabled });
}

export async function adminDeleteFilter(id: number): Promise<void> {
  await request("DELETE", `/admin/filters/${id}`);
}

export async function adminGetQuota(): Promise<QuotaEntry[]> {
  return request("GET", "/admin/quota");
}

// ── Custom Domains ────────────────────────────────────────────

export interface CustomDomain {
  id: number;
  domain: string;
  user_id: number;
  verified: boolean;
  created_at: number;
}

export async function listDomains(): Promise<CustomDomain[]> {
  return request("GET", "/admin/domains");
}

export async function addDomain(
  domain: string,
): Promise<{ ok: boolean; domain: CustomDomain; dns: Record<string, string> }> {
  return request("POST", "/admin/domains", { domain });
}

export async function verifyDomain(
  id: number,
): Promise<{ ok: boolean; verified: boolean; domain: string }> {
  return request("POST", "/admin/domains/verify", { id });
}

export async function deleteDomain(id: number): Promise<void> {
  await request("DELETE", `/admin/domains/${id}`);
}
