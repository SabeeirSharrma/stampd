import { useState, useEffect } from "react";
import {
  getMessage,
  deleteMessage,
  archiveMessage,
  reportSpam,
  markRead,
  type MessageDetail,
} from "../../lib/api";

interface MailViewProps {
  messageId: string | null;
  folder: string;
  onBack: () => void;
  onMessageAction: () => void;
}

export default function MailView({
  messageId,
  folder,
  onBack,
  onMessageAction,
}: MailViewProps) {
  const [message, setMessage] = useState<MessageDetail | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!messageId) {
      setMessage(null);
      return;
    }

    let cancelled = false;

    async function load() {
      setLoading(true);
      setError(null);
      try {
        const msg = await getMessage(messageId!);
        if (!cancelled) {
          setMessage(msg);
          if (folder === "inbox" && msg.folder === "new") {
            try {
              await markRead(messageId!);
            } catch {}
          }
        }
      } catch {
        if (!cancelled) setError("Failed to load message");
      } finally {
        if (!cancelled) setLoading(false);
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [messageId, folder]);

  async function handleDelete() {
    if (!messageId) return;
    try {
      await deleteMessage(messageId);
      onMessageAction();
    } catch {}
  }

  async function handleArchive() {
    if (!messageId) return;
    try {
      await archiveMessage(messageId);
      onMessageAction();
    } catch {}
  }

  async function handleReportSpam() {
    if (!messageId) return;
    try {
      await reportSpam(messageId);
      onMessageAction();
    } catch {}
  }

  function handleReply() {
    if (!message) return;
    const replyTo = folder === "sent" ? message.to : message.from;
    const params = new URLSearchParams({
      to: replyTo,
      subject: `Re: ${message.subject || ""}`,
    });
    window.location.href = `/compose?${params.toString()}`;
  }

  function handleForward() {
    if (!message) return;
    const params = new URLSearchParams({
      subject: `Fwd: ${message.subject || ""}`,
      body: `\n\n---------- Forwarded message ----------\nFrom: ${message.from}\nTo: ${message.to}\nSubject: ${message.subject}\n\n${message.body || ""}`,
    });
    window.location.href = `/compose?${params.toString()}`;
  }

  if (!messageId) {
    return (
      <section className="flex-1 flex flex-col h-screen bg-surface min-w-0">
        <div className="flex-1 flex flex-col items-center justify-center text-on-secondary-container gap-4">
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="48"
            height="48"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeLinejoin="round"
            className="opacity-30"
          >
            <path d="M22 12h-6l-2 3h-4l-2-3H2" />
            <path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z" />
          </svg>
          <p className="text-body-md">Select a message to read</p>
          <p className="text-body-sm opacity-60">
            Use j/k to navigate, Enter to open, r to reply
          </p>
        </div>
      </section>
    );
  }

  if (loading) {
    return (
      <section className="flex-1 flex flex-col h-screen bg-surface min-w-0">
        <div className="flex-1 flex items-center justify-center text-on-secondary-container text-body-md">
          <svg
            className="animate-spin mr-3"
            xmlns="http://www.w3.org/2000/svg"
            width="20"
            height="20"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
          >
            <path d="M21 12a9 9 0 1 1-6.219-8.56" />
          </svg>
          Loading...
        </div>
      </section>
    );
  }

  if (error || !message) {
    return (
      <section className="flex-1 flex flex-col h-screen bg-surface min-w-0">
        <div className="flex-1 flex items-center justify-center text-error text-body-md">
          {error || "Failed to load message"}
        </div>
      </section>
    );
  }

  const date = new Date(message.date);
  const dateStr = date.toLocaleString();
  const subject = message.subject || "(no subject)";
  const bodyHtml = (message.body || "").replace(/\n/g, "<br>");
  const initials = message.from.split("@")[0].slice(0, 2).toUpperCase();
  const isDraft = folder === "drafts";

  return (
    <section className="flex-1 flex flex-col h-screen bg-surface min-w-0">
      <header className="h-16 flex items-center justify-between px-6 border-b border-outline-variant bg-surface-container-lowest shrink-0">
        <div className="flex items-center gap-4">
          <button
            onClick={onBack}
            className="p-2 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors"
            title="Back to list (Esc)"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m15 18-6-6 6-6"/></svg>
          </button>
          {!isDraft && (
            <>
              <button
                onClick={handleArchive}
                className="p-2 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors"
                title="Archive"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 8v13H3V8"/><path d="M1 3h22v5H1z"/><path d="M10 12h4"/></svg>
              </button>
              <button
                onClick={handleReportSpam}
                className="p-2 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors"
                title="Report Spam"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M20 13c0 5-3.5 7.5-7.66 8.95a1 1 0 0 1-.67-.01C7.5 20.5 4 18 4 13V6a1 1 0 0 1 1-1c2 0 4.5-1.2 6.24-2.72a1.17 1.17 0 0 1 1.52 0C14.51 3.81 17 5 19 5a1 1 0 0 1 1 1z"/><path d="M12 8v4"/><path d="M12 16h.01"/></svg>
              </button>
              <button
                onClick={handleDelete}
                className="p-2 hover:bg-surface-container rounded-full text-error transition-colors"
                title="Delete (#)"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
              </button>
            </>
          )}
        </div>
      </header>

      <div className="flex-1 overflow-y-auto custom-scrollbar p-6 bg-surface">
        <div className="max-w-3xl mx-auto">
          <h2 className="text-display-sm font-semibold mb-8 text-on-surface tracking-tight">
            {subject}
          </h2>
          <div className="flex items-center justify-between mb-10">
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-full flex items-center justify-center bg-primary text-on-primary font-bold text-lg">
                {initials}
              </div>
              <div>
                <p className="font-bold text-on-surface">
                  {isDraft
                    ? message.to || "(no recipient)"
                    : message.from.split("@")[0]}
                  <span className="font-normal text-on-secondary-container ml-2">
                    &lt;{isDraft ? message.to || "" : message.from}&gt;
                  </span>
                </p>
                <p className="text-body-sm text-on-secondary-container">
                  {isDraft ? "Draft" : `to ${message.to}`}
                </p>
              </div>
            </div>
            <div className="text-right">
              <p className="font-mono text-mono-sm text-on-secondary-container">
                {dateStr}
              </p>
            </div>
          </div>
          <div
            className="space-y-6 text-on-surface-variant leading-relaxed text-body-lg"
            dangerouslySetInnerHTML={{ __html: bodyHtml }}
          />
        </div>
      </div>

      <footer className="p-6 border-t border-outline-variant bg-surface-container-lowest shrink-0">
        <div className="max-w-3xl mx-auto flex gap-4">
          {isDraft ? (
            <>
              <a
                href={`/compose?draft=${message.id}`}
                className="flex-1 flex items-center justify-center gap-2 border border-outline-variant py-3 rounded-lg hover:bg-surface-container transition-colors font-bold text-primary"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/></svg>
                Edit
              </a>
            </>
          ) : (
            <>
              <button
                onClick={handleReply}
                className="flex-1 flex items-center justify-center gap-2 border border-outline-variant py-3 rounded-lg hover:bg-surface-container transition-colors font-bold text-primary"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M9 17H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v3"/><path d="m3 15 4 4 4-4"/><path d="M7 8v8l4-4-4-4z"/></svg>
                Reply
              </button>
              <button
                onClick={handleForward}
                className="flex-1 flex items-center justify-center gap-2 border border-outline-variant py-3 rounded-lg hover:bg-surface-container transition-colors font-bold text-primary"
              >
                <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M15 17H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2v3"/><path d="m14 4 4 4-4 4"/><path d="M17 8V4l4 4-4 4"/></svg>
                Forward
              </button>
            </>
          )}
        </div>
      </footer>
    </section>
  );
}
