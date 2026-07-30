import { useState, useEffect, useCallback } from "react";
import {
  listMessages,
  deleteMessage,
  archiveMessage,
  unarchiveMessage,
  reportSpam,
  notSpam,
  sendDraft,
  type MailboxMessage,
} from "../../lib/api";

interface MailListProps {
  folder: string;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRefresh?: () => void;
}

export default function MailList({
  folder,
  selectedId,
  onSelect,
  onRefresh,
}: MailListProps) {
  const [messages, setMessages] = useState<MailboxMessage[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [showSearch, setShowSearch] = useState(false);

  const loadMessages = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const { messages: msgs } = await listMessages(folder as any);
      setMessages(msgs);
    } catch {
      setError("Failed to load messages");
    } finally {
      setLoading(false);
    }
  }, [folder]);

  useEffect(() => {
    loadMessages();
  }, [loadMessages]);

  useEffect(() => {
    const handler = () => {
      if (!document.hidden) loadMessages();
    };
    document.addEventListener("visibilitychange", handler);
    return () => document.removeEventListener("visibilitychange", handler);
  }, [loadMessages]);

  const filtered = searchQuery
    ? messages.filter(
        (m) =>
          m.subject.toLowerCase().includes(searchQuery) ||
          m.from.toLowerCase().includes(searchQuery) ||
          m.to.toLowerCase().includes(searchQuery),
      )
    : messages;

  async function handleDelete(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await deleteMessage(id);
      setMessages((prev) => prev.filter((m) => m.id !== id));
      if (selectedId === id) onSelect("");
    } catch {}
  }

  async function handleArchive(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await archiveMessage(id);
      setMessages((prev) => prev.filter((m) => m.id !== id));
      if (selectedId === id) onSelect("");
    } catch {}
  }

  async function handleUnarchive(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await unarchiveMessage(id);
      setMessages((prev) => prev.filter((m) => m.id !== id));
      if (selectedId === id) onSelect("");
    } catch {}
  }

  async function handleNotSpam(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await notSpam(id);
      setMessages((prev) => prev.filter((m) => m.id !== id));
      if (selectedId === id) onSelect("");
    } catch {}
  }

  async function handleSendDraft(id: string, e: React.MouseEvent) {
    e.stopPropagation();
    try {
      await sendDraft(id);
      setMessages((prev) => prev.filter((m) => m.id !== id));
    } catch {}
  }

  function getInitials(msg: MailboxMessage): string {
    const addr = folder === "sent" ? msg.to : msg.from;
    return addr.split("@")[0].slice(0, 2).toUpperCase();
  }

  function getNameDisplay(msg: MailboxMessage): string {
    if (folder === "sent") return `To: ${msg.to.split("@")[0]}`;
    if (folder === "drafts") return `To: ${msg.to || "(no recipient)"}`;
    return msg.from.split("@")[0];
  }

  function getTimeStr(dateStr: string): string {
    const d = new Date(dateStr);
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  }

  return (
    <main className="flex flex-col h-screen w-[360px] bg-surface-container-low border-r border-outline-variant z-30 shrink-0">
      <div className="h-16 flex items-center justify-between px-6 border-b border-outline-variant">
        <h1 className="text-headline-sm font-semibold text-on-surface capitalize">
          {folder}
        </h1>
        <div className="flex gap-2">
          <button
            onClick={loadMessages}
            className="p-1.5 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors"
            title="Refresh"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8"/><path d="M3 3v5h5"/><path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16"/><path d="M16 16h5v5"/></svg>
          </button>
          <button
            onClick={() => {
              setShowSearch(!showSearch);
              if (showSearch) setSearchQuery("");
            }}
            className="p-1.5 hover:bg-surface-container rounded-full text-on-surface-variant transition-colors"
            title="Search"
          >
            <svg xmlns="http://www.w3.org/2000/svg" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>
          </button>
        </div>
      </div>

      {showSearch && (
        <div className="px-6 py-3 border-b border-outline-variant bg-surface-container-low">
          <input
            type="text"
            placeholder="Search messages..."
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            autoFocus
            className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-2.5 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors"
          />
        </div>
      )}

      <div className="flex-1 overflow-y-auto custom-scrollbar">
        {loading ? (
          <div className="flex items-center justify-center h-full text-on-secondary-container text-body-md">
            Loading messages...
          </div>
        ) : error ? (
          <div className="flex flex-col items-center justify-center h-full gap-4 text-on-secondary-container">
            <p className="text-body-md">{error}</p>
            <p className="text-body-sm">Is the gateway running?</p>
            <button
              onClick={loadMessages}
              className="mt-2 px-4 py-2 bg-primary-container text-on-primary-container rounded-lg text-label-md font-bold hover:opacity-90"
            >
              Retry
            </button>
          </div>
        ) : filtered.length === 0 ? (
          <div className="flex items-center justify-center h-full text-on-secondary-container text-body-md">
            {searchQuery ? "No matching messages" : "No messages"}
          </div>
        ) : (
          filtered.map((msg) => {
            const isSelected = msg.id === selectedId;
            const subject = msg.subject || "(no subject)";

            return (
              <div
                key={msg.id}
                onClick={() => onSelect(msg.id)}
                className={`relative group p-6 border-b border-outline-variant cursor-pointer transition-colors ${
                  isSelected
                    ? "bg-surface-container hover:bg-surface-container-high"
                    : "hover:bg-surface-container"
                }`}
              >
                <div className="flex gap-4 mb-2">
                  <div className="w-10 h-10 rounded-full flex items-center justify-center bg-secondary-container text-on-secondary-container font-bold text-xs shrink-0">
                    {getInitials(msg)}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="flex justify-between items-start">
                      <p
                        className={`truncate ${
                          isSelected ? "font-bold" : "font-semibold"
                        } text-on-surface`}
                      >
                        {getNameDisplay(msg)}
                      </p>
                      <span className="font-mono text-[10px] text-on-secondary-container shrink-0 ml-2">
                        {getTimeStr(msg.date)}
                      </span>
                    </div>
                    <p
                      className={`text-body-md truncate ${
                        isSelected
                          ? "text-primary font-medium"
                          : "text-on-surface-variant"
                      }`}
                    >
                      Subject: {subject}
                    </p>
                  </div>
                </div>

                <div className="absolute right-4 top-1/2 -translate-y-1/2 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity bg-surface-container-highest p-1 rounded-lg">
                  {folder === "drafts" ? (
                    <>
                      <a
                        href={`/compose?draft=${msg.id}`}
                        onClick={(e) => e.stopPropagation()}
                        className="p-1.5 hover:bg-surface-bright rounded transition-colors text-primary"
                        title="Edit"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M17 3a2.85 2.83 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5Z"/><path d="m15 5 4 4"/></svg>
                      </a>
                      <button
                        onClick={(e) => handleSendDraft(msg.id, e)}
                        className="p-1.5 hover:bg-surface-bright rounded transition-colors text-primary"
                        title="Send"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="m22 2-7 20-4-9-9-4Z"/><path d="M22 2 11 13"/></svg>
                      </button>
                    </>
                  ) : folder === "archive" ? (
                    <button
                      onClick={(e) => handleUnarchive(msg.id, e)}
                      className="p-1.5 hover:bg-surface-bright rounded transition-colors text-primary"
                      title="Unarchive"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 8v13H3V8"/><path d="M1 3h22v5H1z"/><path d="m12 3 9 5H3z"/></svg>
                    </button>
                  ) : folder === "spam" ? (
                    <button
                      onClick={(e) => handleNotSpam(msg.id, e)}
                      className="p-1.5 hover:bg-surface-bright rounded transition-colors text-primary"
                      title="Not Spam"
                    >
                      <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/></svg>
                    </button>
                  ) : (
                    <>
                      <button
                        onClick={(e) => handleArchive(msg.id, e)}
                        className="p-1.5 hover:bg-surface-bright rounded transition-colors text-primary"
                        title="Archive"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M21 8v13H3V8"/><path d="M1 3h22v5H1z"/><path d="M10 12h4"/></svg>
                      </button>
                      <button
                        onClick={(e) => handleDelete(msg.id, e)}
                        className="p-1.5 hover:bg-surface-bright rounded transition-colors text-error"
                        title="Delete"
                      >
                        <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/></svg>
                      </button>
                    </>
                  )}
                </div>
              </div>
            );
          })
        )}
      </div>
    </main>
  );
}
