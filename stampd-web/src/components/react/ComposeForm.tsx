import { useState, useEffect, type FormEvent } from "react";
import {
  sendMessage,
  saveDraft,
  updateDraft,
  deleteDraft,
  getMessage,
  logout as apiLogout,
} from "../../lib/api";
import { setUser } from "../../lib/auth";

interface ComposeFormProps {
  draftId?: string | null;
  presetTo?: string;
  presetSubject?: string;
  presetBody?: string;
}

export default function ComposeForm({
  draftId,
  presetTo,
  presetSubject,
  presetBody,
}: ComposeFormProps) {
  const [to, setTo] = useState("");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [saving, setSaving] = useState(false);
  const [pageTitle, setPageTitle] = useState("New Message");
  const [currentDraftId, setCurrentDraftId] = useState<string | null>(
    draftId || null,
  );

  useEffect(() => {
    async function loadDraft() {
      if (!draftId) {
        if (presetTo) setTo(presetTo);
        if (presetSubject) setSubject(presetSubject);
        if (presetBody) setBody(presetBody);
        return;
      }
      try {
        const draft = await getMessage(draftId);
        if (draft.to) setTo(draft.to);
        if (draft.subject) setSubject(draft.subject);
        if (draft.body) setBody(draft.body);
        setPageTitle("Edit Draft");
      } catch {
        setCurrentDraftId(null);
      }
    }
    loadDraft();
  }, [draftId, presetTo, presetSubject, presetBody]);

  useEffect(() => {
    const timer = setInterval(async () => {
      if (!to && !subject && !body) return;
      try {
        const draftData = { to, subject, body };
        if (currentDraftId) {
          await updateDraft(currentDraftId, draftData);
        } else {
          const saved = await saveDraft(draftData);
          setCurrentDraftId(saved.id);
        }
      } catch {}
    }, 30000);
    return () => clearInterval(timer);
  }, [to, subject, body, currentDraftId]);

  async function handleSend(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setSending(true);
    try {
      await sendMessage(to, subject, body);
      if (currentDraftId) {
        try { await deleteDraft(currentDraftId); } catch {}
      }
      window.location.href = "/inbox?sent=1";
    } catch (err: any) {
      setError(err.message || "Failed to send");
    } finally {
      setSending(false);
    }
  }

  async function handleSaveDraft() {
    setSaving(true);
    try {
      const draftData = { to, subject, body };
      if (currentDraftId) {
        await updateDraft(currentDraftId, draftData);
      } else {
        await saveDraft(draftData);
      }
      window.location.href = "/inbox";
    } catch (err: any) {
      setError(err.message || "Failed to save draft");
    } finally {
      setSaving(false);
    }
  }

  async function handleDiscard() {
    if (currentDraftId) {
      try { await deleteDraft(currentDraftId); } catch {}
    }
    window.location.href = "/inbox";
  }

  async function handleLogout() {
    try { await apiLogout(); } catch {}
    setUser(null);
    window.location.href = "/";
  }

  const backIcon = (
    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m15 18-6-6 6-6"/></svg>
  );
  const sendIcon = (
    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m22 2-7 20-4-9-9-4Z"/><path d="M22 2 11 13"/></svg>
  );
  const saveIcon = (
    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z"/><path d="M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7"/><path d="M7 3v4a1 1 0 0 0 1 1h7"/></svg>
  );
  const logoutIcon = (
    <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/></svg>
  );

  return (
    <div className="min-h-screen bg-background">
      <header className="h-16 flex items-center justify-between px-6 border-b border-outline-variant bg-surface-container-lowest">
        <div className="flex items-center gap-4">
          <a href="/inbox" className="p-2 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors">
            {backIcon}
          </a>
          <h1 className="text-headline-sm font-semibold text-on-surface">{pageTitle}</h1>
        </div>
        <button onClick={handleLogout} className="p-2 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors">
          {logoutIcon}
        </button>
      </header>

      <div className="max-w-3xl mx-auto p-6">
        <form onSubmit={handleSend} className="space-y-5">
          <div>
            <label htmlFor="to" className="block text-label-md text-on-surface-variant mb-1.5">To</label>
            <input id="to" type="email" required placeholder="recipient@example.com" value={to} onChange={(e) => setTo(e.target.value)} className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors" />
          </div>

          <div>
            <label htmlFor="subject" className="block text-label-md text-on-surface-variant mb-1.5">Subject</label>
            <input id="subject" type="text" placeholder="What's this about?" value={subject} onChange={(e) => setSubject(e.target.value)} className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors" />
          </div>

          <div>
            <label htmlFor="body" className="block text-label-md text-on-surface-variant mb-1.5">Message</label>
            <textarea id="body" required rows={16} placeholder="Write your message..." value={body} onChange={(e) => setBody(e.target.value)} className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors resize-y min-h-[300px]" />
          </div>

          {error && <p className="text-body-sm text-error">{error}</p>}

          <div className="flex gap-4 pt-2">
            <button type="submit" disabled={sending} className="flex-1 flex items-center justify-center gap-2 bg-primary-container text-on-primary-container py-3 rounded-lg font-bold text-label-md hover:opacity-90 active:scale-[0.98] transition-all disabled:opacity-50">
              {sendIcon} {sending ? "Sending..." : "Send"}
            </button>
            <button type="button" onClick={handleSaveDraft} disabled={saving} className="flex-1 flex items-center justify-center gap-2 border border-outline-variant rounded-lg font-bold text-label-md text-on-surface-variant hover:bg-surface-container transition-colors py-3 disabled:opacity-50">
              {saveIcon} {saving ? "Saving..." : "Save Draft"}
            </button>
            <button type="button" onClick={handleDiscard} className="px-6 py-3 border border-outline-variant rounded-lg font-bold text-label-md text-on-surface-variant hover:bg-surface-container transition-colors">
              Discard
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
