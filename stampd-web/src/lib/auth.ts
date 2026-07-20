/**
 * Client-side auth state management.
 * Reads/writes a simple localStorage flag + redirects on 401.
 */

export interface AuthUser {
  id: number;
  email: string;
  is_admin: boolean;
}

const STORAGE_KEY = "stampd_user";

export function getUser(): AuthUser | null {
  if (typeof window === "undefined") return null;
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw) as AuthUser;
  } catch {
    return null;
  }
}

export function setUser(user: AuthUser | null) {
  if (typeof window === "undefined") return;
  if (user) {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(user));
  } else {
    localStorage.removeItem(STORAGE_KEY);
  }
}

export function isAdmin(): boolean {
  return getUser()?.is_admin === true;
}

/**
 * Redirect to /login if not authenticated.
 * Call this at the top of protected pages.
 */
export function requireAuth(): AuthUser {
  const user = getUser();
  if (!user) {
    window.location.href = "/login";
    throw new Error("Not authenticated");
  }
  return user;
}

/**
 * Redirect to / if already authenticated.
 */
export function redirectIfAuth(): AuthUser | null {
  const user = getUser();
  if (user) {
    window.location.href = "/";
    return user;
  }
  return null;
}
