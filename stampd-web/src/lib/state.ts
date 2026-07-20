/**
 * Lightweight reactive state for the mail UI.
 * Pub/sub event system + typed state store.
 */

type Listener<T> = (value: T, prev: T) => void;

export class Signal<T> {
  private _value: T;
  private _listeners: Listener<T>[] = [];

  constructor(initial: T) {
    this._value = initial;
  }

  get value(): T {
    return this._value;
  }

  set(next: T) {
    const prev = this._value;
    if (Object.is(prev, next)) return;
    this._value = next;
    for (const fn of this._listeners) fn(next, prev);
  }

  update(fn: (prev: T) => T) {
    this.set(fn(this._value));
  }

  subscribe(fn: Listener<T>): () => void {
    this._listeners.push(fn);
    return () => {
      this._listeners = this._listeners.filter((l) => l !== fn);
    };
  }
}

// ── App State ───────────────────────────────────────────────

import type { MailboxMessage, MessageDetail } from "./api";

export interface AppState {
  // Messages
  messages: Signal<MailboxMessage[]>;
  selectedId: Signal<string | null>;
  selectedMessage: Signal<MessageDetail | null>;

  // UI state
  loading: Signal<boolean>;
  error: Signal<string | null>;
  view: Signal<"list" | "reading">;

  // Folder
  folder: Signal<string>;
}

function createAppState(): AppState {
  return {
    messages: new Signal<MailboxMessage[]>([]),
    selectedId: new Signal<string | null>(null),
    selectedMessage: new Signal<MessageDetail | null>(null),
    loading: new Signal(true),
    error: new Signal<string | null>(null),
    view: new Signal<"list" | "reading">("list"),
    folder: new Signal("inbox"),
  };
}

export const state = createAppState();

// ── Derived helpers ─────────────────────────────────────────

export function selectedMessage(): MessageDetail | null {
  return state.selectedMessage.value;
}

export function isSelected(id: string): boolean {
  return state.selectedId.value === id;
}

export function messages(): MailboxMessage[] {
  return state.messages.value;
}
