/**
 * Transit-powered filter bridge.
 *
 * Starts the Python resident process and exposes filter functions
 * that the gateway calls during SMTP processing.
 */

import { transit } from "@sabeeirsharrma/transit";
import { resolve } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));
const PYTHON_DIR = resolve(__dirname, "../python/filters");

let py: ReturnType<typeof transit.python> | null = null;

/**
 * Initialize the Transit Python bridge.
 * Call this once on gateway startup.
 */
export function initFilters() {
  if (!existsSync(PYTHON_DIR)) {
    console.warn(`[filters] Python directory not found: ${PYTHON_DIR}`);
    return false;
  }

  try {
    py = transit.python(PYTHON_DIR);
    console.log("[filters] Transit Python bridge initialized");
    transit.info();
    return true;
  } catch (err) {
    console.error("[filters] Failed to initialize Transit Python bridge:", err);
    return false;
  }
}

/**
 * Run a filter function via Transit.
 *
 * @param functionName - Name of the Python function (camelCase)
 * @param context - Filter context (sender, recipient, hook, etc.)
 * @returns Filter result: { action: "accept" | "reject", reason: string }
 */
export async function runFilter(
  functionName: string,
  context: Record<string, unknown>
): Promise<{ action: string; reason: string }> {
  if (!py) {
    // Bridge not initialized — accept by default
    return { action: "accept", reason: "Filter bridge not available" };
  }

  try {
    const fn = (py as any)[functionName];
    if (!fn) {
      console.warn(`[filters] Unknown filter function: ${functionName}`);
      return { action: "accept", reason: `Unknown function: ${functionName}` };
    }

    // Transit auto-stringifies args, so pass the object directly
    const result = await fn(context);
    return typeof result === "string" ? JSON.parse(result) : result;
  } catch (err) {
    console.error(`[filters] Error running ${functionName}:`, err);
    // On error, accept (don't block mail delivery)
    return { action: "accept", reason: `Filter error: ${err}` };
  }
}

/**
 * Run all enabled filters for a given hook point.
 *
 * @param hook - Hook point: "mail_from" | "rcpt_to" | "data"
 * @param context - Base filter context
 * @param enabledFilters - List of enabled filter function names for this hook
 * @returns Ok if all accept, Err(reason) if any rejects
 */
export async function runFiltersForHook(
  hook: string,
  context: Record<string, unknown>,
  enabledFilters: string[]
): Promise<{ ok: true } | { ok: false; reason: string }> {
  const hookContext = { ...context, hook };

  for (const filterName of enabledFilters) {
    const result = await runFilter(filterName, hookContext);
    if (result.action === "reject") {
      return { ok: false, reason: result.reason };
    }
  }

  return { ok: true };
}

/**
 * Stop the Python resident process.
 * Call this on gateway shutdown.
 */
export async function stopFilters() {
  if (py) {
    try {
      await (py as any)._bridge?.stop();
    } catch {}
    py = null;
  }
}
