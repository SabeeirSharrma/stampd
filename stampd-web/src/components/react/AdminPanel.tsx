import { useState, useEffect, useCallback } from "react";
import {
  adminListUsers,
  adminDisableUser,
  adminDeleteUser,
  adminGetConfig,
  adminUpdateConfig,
  adminListQueue,
  adminRetryMessage,
  adminPurgeMessage,
  adminListLogs,
  adminListFilters,
  adminCreateFilter,
  adminToggleFilter,
  adminDeleteFilter,
  type AdminUser,
  type ServerConfig,
  type QueueMessage,
  type DeliveryLog,
  type FilterRecord,
} from "../../lib/api";

type Tab = "users" | "config" | "queue" | "logs" | "filters";

export default function AdminPanel() {
  const [tab, setTab] = useState<Tab>("users");

  return (
    <div className="max-w-4xl mx-auto p-6 space-y-8">
      <nav className="flex gap-1 border-b border-outline-variant">
        {(["users", "config", "queue", "logs", "filters"] as Tab[]).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`px-4 py-3 text-label-md capitalize transition-colors ${
              tab === t
                ? "text-primary font-bold border-b-2 border-primary"
                : "text-on-secondary-container hover:text-on-surface"
            }`}
          >
            {t}
          </button>
        ))}
      </nav>

      {tab === "users" && <UsersTab />}
      {tab === "config" && <ConfigTab />}
      {tab === "queue" && <QueueTab />}
      {tab === "logs" && <LogsTab />}
      {tab === "filters" && <FiltersTab />}
    </div>
  );
}

function UsersTab() {
  const [users, setUsers] = useState<AdminUser[]>([]);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    try {
      setUsers(await adminListUsers());
    } catch {} finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  if (loading) return <p className="text-on-secondary-container">Loading...</p>;

  return (
    <div className="bg-surface-container-low border border-outline-variant rounded-xl p-6">
      <div className="space-y-3">
        {users.map((u) => (
          <div key={u.id} className="flex items-center justify-between p-4 bg-surface-container rounded-lg border border-outline-variant">
            <div>
              <p className="text-body-md text-on-surface font-medium">{u.email}</p>
              <p className="text-body-sm text-on-secondary-container">
                {u.is_admin ? "Admin" : "User"} {u.disabled ? "· Disabled" : ""}
              </p>
            </div>
            <div className="flex gap-2">
              {!u.disabled && (
                <button onClick={async () => { await adminDisableUser(u.id); load(); }} className="text-body-sm text-error hover:underline">Disable</button>
              )}
              <button onClick={async () => { if (confirm("Delete this user? This cannot be undone.")) { await adminDeleteUser(u.id); load(); } }} className="text-body-sm text-error hover:underline">Delete</button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function ConfigTab() {
  const [config, setConfig] = useState<ServerConfig | null>(null);
  const [domain, setDomain] = useState("");
  const [dkim, setDkim] = useState("");
  const [signup, setSignup] = useState(true);
  const [msg, setMsg] = useState<{ text: string; ok: boolean } | null>(null);

  useEffect(() => {
    adminGetConfig().then((cfg) => {
      setConfig(cfg);
      setDomain(cfg.domain);
      setDkim(cfg.dkim_selector);
      setSignup(cfg.signup_enabled === 1);
    }).catch(() => {});
  }, []);

  async function handleSave() {
    try {
      const result = await adminUpdateConfig({ domain, dkim_selector: dkim, signup_enabled: signup });
      setConfig(result.config);
      setMsg({ text: "Config saved", ok: true });
    } catch (err: any) {
      setMsg({ text: err.message || "Failed", ok: false });
    }
  }

  if (!config) return <p className="text-on-secondary-container">Loading...</p>;

  return (
    <div className="bg-surface-container-low border border-outline-variant rounded-xl p-6 space-y-5">
      <div>
        <label className="block text-label-md text-on-surface-variant mb-1.5">Domain</label>
        <input value={domain} onChange={(e) => setDomain(e.target.value)} className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors" />
      </div>
      <div>
        <label className="block text-label-md text-on-surface-variant mb-1.5">DKIM Selector</label>
        <input value={dkim} onChange={(e) => setDkim(e.target.value)} className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors" />
      </div>
      <div className="flex items-center gap-3">
        <input type="checkbox" checked={signup} onChange={(e) => setSignup(e.target.checked)} className="w-4 h-4 rounded border-outline-variant text-primary focus:ring-primary/20" />
        <label className="text-body-md text-on-surface">Allow self-signup</label>
      </div>
      <button onClick={handleSave} className="px-6 py-3 bg-primary-container text-on-primary-container rounded-lg font-bold text-label-md hover:opacity-90 transition-opacity">Save Config</button>
      {msg && <p className={`text-body-sm ${msg.ok ? "text-primary" : "text-error"}`}>{msg.text}</p>}
    </div>
  );
}

function QueueTab() {
  const [messages, setMessages] = useState<QueueMessage[]>([]);
  const [filter, setFilter] = useState("");
  const [loading, setLoading] = useState(true);

  const load = useCallback(async (status?: string) => {
    setLoading(true);
    try {
      setMessages(await adminListQueue(status));
    } catch {} finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(filter || undefined); }, [load, filter]);

  return (
    <>
      <div className="flex gap-3 mb-4">
        <select value={filter} onChange={(e) => setFilter(e.target.value)} className="bg-surface-container border border-outline-variant rounded-lg px-4 py-2.5 text-body-md text-on-surface focus:border-primary outline-none">
          <option value="">All</option>
          <option value="pending">Pending</option>
          <option value="delivered">Delivered</option>
          <option value="dead">Dead</option>
        </select>
      </div>
      <div className="bg-surface-container-low border border-outline-variant rounded-xl p-6">
        <div className="space-y-3">
          {loading ? (
            <p className="text-on-secondary-container">Loading...</p>
          ) : messages.length === 0 ? (
            <p className="text-body-sm text-on-secondary-container">No messages</p>
          ) : (
            messages.map((m) => (
              <div key={m.id} className="flex items-center justify-between p-4 bg-surface-container rounded-lg border border-outline-variant">
                <div className="flex-1 min-w-0">
                  <p className="text-body-md text-on-surface font-medium truncate">{m.recipient}</p>
                  <p className="text-body-sm text-on-secondary-container">
                    Status: {m.status} · Attempts: {m.attempts}{m.last_error ? ` · Error: ${m.last_error}` : ""}
                  </p>
                </div>
                <div className="flex gap-2 ml-4">
                  {m.status === "dead" && (
                    <button onClick={async () => { await adminRetryMessage(m.id); load(filter || undefined); }} className="text-body-sm text-primary hover:underline">Retry</button>
                  )}
                  <button onClick={async () => { await adminPurgeMessage(m.id); load(filter || undefined); }} className="text-body-sm text-error hover:underline">Purge</button>
                </div>
              </div>
            ))
          )}
        </div>
      </div>
    </>
  );
}

function LogsTab() {
  const [logs, setLogs] = useState<DeliveryLog[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    adminListLogs({ limit: 50 }).then(setLogs).catch(() => {}).finally(() => setLoading(false));
  }, []);

  return (
    <div className="bg-surface-container-low border border-outline-variant rounded-xl p-6">
      <div className="space-y-3">
        {loading ? (
          <p className="text-on-secondary-container">Loading...</p>
        ) : logs.length === 0 ? (
          <p className="text-body-sm text-on-secondary-container">No logs</p>
        ) : (
          logs.map((l) => (
            <div key={l.id} className="p-3 bg-surface-container rounded-lg border border-outline-variant">
              <div className="flex justify-between items-start">
                <p className="text-body-md text-on-surface">{l.recipient}</p>
                <span className={`text-mono-sm ${l.status === "delivered" ? "text-primary" : "text-error"}`}>{l.status}</span>
              </div>
              {l.error && <p className="text-body-sm text-error mt-1">{l.error}</p>}
            </div>
          ))
        )}
      </div>
    </div>
  );
}

function FiltersTab() {
  const [filters, setFilters] = useState<FilterRecord[]>([]);
  const [loading, setLoading] = useState(true);
  const [name, setName] = useState("");
  const [path, setPath] = useState("");

  const load = useCallback(async () => {
    try {
      setFilters(await adminListFilters());
    } catch {} finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  return (
    <div className="bg-surface-container-low border border-outline-variant rounded-xl p-6">
      <div className="space-y-3 mb-6">
        {loading ? (
          <p className="text-on-secondary-container">Loading...</p>
        ) : filters.length === 0 ? (
          <p className="text-body-sm text-on-secondary-container">No filters configured</p>
        ) : (
          filters.map((f) => (
            <div key={f.id} className="flex items-center justify-between p-4 bg-surface-container rounded-lg border border-outline-variant">
              <div>
                <p className="text-body-md text-on-surface font-medium">{f.name}</p>
                <p className="text-body-sm text-on-secondary-container">{f.hooks} · {f.enabled ? "Enabled" : "Disabled"}</p>
              </div>
              <div className="flex gap-3 items-center">
                <button onClick={async () => { await adminToggleFilter(f.id, !f.enabled); load(); }} className={`text-body-sm ${f.enabled ? "text-error" : "text-primary"} hover:underline`}>
                  {f.enabled ? "Disable" : "Enable"}
                </button>
                <button onClick={async () => { if (confirm("Delete this filter?")) { await adminDeleteFilter(f.id); load(); } }} className="text-body-sm text-error hover:underline">Delete</button>
              </div>
            </div>
          ))
        )}
      </div>
      <div className="border-t border-outline-variant pt-6">
        <h3 className="text-label-md font-bold text-on-surface mb-3">Add Filter</h3>
        <div className="flex gap-3">
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Name" className="flex-1 bg-surface-container border border-outline-variant rounded-lg px-4 py-2.5 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary outline-none" />
          <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="Path" className="flex-1 bg-surface-container border border-outline-variant rounded-lg px-4 py-2.5 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary outline-none" />
          <button onClick={async () => { if (!name || !path) return; await adminCreateFilter(name, path, ["mail_from"]); setName(""); setPath(""); load(); }} className="px-4 py-2.5 bg-primary-container text-on-primary-container rounded-lg font-bold text-label-md hover:opacity-90 transition-opacity">Add</button>
        </div>
      </div>
    </div>
  );
}
