import { useState, useEffect, useCallback } from "react";
import MailList from "./MailList";
import MailView from "./MailView";
import {
  listMessages,
  type MailboxMessage,
} from "../../lib/api";

interface MailAppProps {
  initialFolder?: string;
}

export default function MailApp({ initialFolder }: MailAppProps) {
  const [folder, setFolder] = useState(() => {
    if (initialFolder) return initialFolder;
    const params = new URLSearchParams(window.location.search);
    return params.get("folder") || "inbox";
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [refreshKey, setRefreshKey] = useState(0);

  // Sync folder from URL
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const f = params.get("folder");
    if (f && f !== folder) setFolder(f);
  }, []);

  const refresh = useCallback(() => {
    setRefreshKey((k) => k + 1);
  }, []);

  // Keyboard navigation
  useEffect(() => {
    let messages: MailboxMessage[] = [];

    // Fetch messages for keyboard nav
    async function loadForNav() {
      try {
        const { messages: msgs } = await listMessages(folder as any);
        messages = msgs;
      } catch {}
    }

    loadForNav();

    // Refresh on folder change
    const interval = setInterval(loadForNav, 5000);

    function handleKeyDown(e: KeyboardEvent) {
      const target = e.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;

      if (messages.length === 0) return;

      const currentIndex = selectedId
        ? messages.findIndex((m) => m.id === selectedId)
        : -1;

      if (e.key === "j" || e.key === "ArrowDown") {
        e.preventDefault();
        const next = currentIndex + 1;
        if (next < messages.length) setSelectedId(messages[next].id);
      } else if (e.key === "k" || e.key === "ArrowUp") {
        e.preventDefault();
        const prev = currentIndex - 1;
        if (prev >= 0) setSelectedId(messages[prev].id);
      } else if (e.key === "Escape") {
        setSelectedId(null);
      } else if (e.key === "r" && selectedId) {
        // Reply - handled by MailView
      }
    }

    document.addEventListener("keydown", handleKeyDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      clearInterval(interval);
    };
  }, [folder, selectedId]);

  return (
    <>
      <MailList
        key={`${folder}-${refreshKey}`}
        folder={folder}
        selectedId={selectedId}
        onSelect={setSelectedId}
        onRefresh={refresh}
      />
      <MailView
        messageId={selectedId}
        folder={folder}
        onBack={() => setSelectedId(null)}
        onMessageAction={() => {
          setSelectedId(null);
          refresh();
        }}
      />
    </>
  );
}
