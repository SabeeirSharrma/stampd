import { useState, type FormEvent } from "react";
import { login, type ApiError } from "../../lib/api";
import { setUser } from "../../lib/auth";

export default function LoginForm() {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);

    try {
      const user = await login(email, password);
      setUser(user);
      window.location.href = "/inbox";
    } catch (err: unknown) {
      const apiErr = err as ApiError;
      setError(apiErr.message || "Login failed");
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="min-h-screen flex items-center justify-center bg-background px-4">
      <div className="w-full max-w-sm">
        <div className="text-center mb-10">
          <h1 className="text-headline-lg font-bold text-primary tracking-tight">
            Stampd
          </h1>
          <p className="text-body-md text-on-secondary-container mt-2">
            Secure Mail
          </p>
        </div>

        <div className="bg-surface-container-low border border-outline-variant rounded-xl p-8">
          <h2 className="text-headline-sm font-semibold text-on-surface mb-6">
            Log in to your account
          </h2>

          <form onSubmit={handleSubmit} className="space-y-5">
            <div>
              <label
                htmlFor="email"
                className="block text-label-md text-on-surface-variant mb-1.5"
              >
                Email
              </label>
              <input
                id="email"
                type="email"
                required
                autoComplete="email"
                placeholder="you@domain.com"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
                className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors"
              />
            </div>

            <div>
              <label
                htmlFor="password"
                className="block text-label-md text-on-surface-variant mb-1.5"
              >
                Password
              </label>
              <input
                id="password"
                type="password"
                required
                autoComplete="current-password"
                placeholder="Enter your password"
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                className="w-full bg-surface-container border border-outline-variant rounded-lg px-4 py-3 text-body-md text-on-surface placeholder:text-on-secondary-container/50 focus:border-primary focus:ring-2 focus:ring-primary/20 outline-none transition-colors"
              />
            </div>

            {error && (
              <p className="text-body-sm text-error">{error}</p>
            )}

            <button
              type="submit"
              disabled={loading}
              className="w-full bg-primary-container text-on-primary-container py-3 rounded-lg font-bold text-label-md hover:opacity-90 active:scale-[0.98] transition-all disabled:opacity-50"
            >
              {loading ? "Logging in..." : "Log In"}
            </button>
          </form>
        </div>

        <p className="text-center text-body-sm text-on-secondary-container mt-6">
          Don't have an account?{" "}
          <a href="/signup" className="text-primary font-bold hover:underline">
            Sign up
          </a>
        </p>
      </div>
    </div>
  );
}
