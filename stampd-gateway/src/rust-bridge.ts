/**
 * Transit Rust bridge for Stampd engine.
 *
 * Loads the engine's napi addon via Transit for zero-overhead function calls.
 * Replaces the axum HTTP API with direct in-process calls.
 */

import { transit } from "@sabeeirsharrma/transit";
import { resolve } from "node:path";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";

const __dirname = fileURLToPath(new URL(".", import.meta.url));

let db: any = null;

/**
 * Initialize the Rust bridge.
 * Call this once on gateway startup.
 */
export function initRustBridge(dbPath: string): boolean {
  // Look for the compiled .node file
  const bridgeDir = resolve(__dirname, "../../stampd-engine-bridge");
  const possiblePaths = [
    resolve(bridgeDir, "index.node"),
    resolve(bridgeDir, "stampd_engine_bridge.node"),
    resolve(bridgeDir, "target/release/stampd_engine_bridge.node"),
    resolve(bridgeDir, "target/release/libstampd_engine_bridge.so"),
  ];

  let nodePath: string | null = null;
  for (const p of possiblePaths) {
    if (existsSync(p)) {
      nodePath = p;
      break;
    }
  }

  if (!nodePath) {
    console.warn("[rust-bridge] No compiled .node file found, using fallback");
    return false;
  }

  try {
    // Load the native addon directly (not via Transit's scanner)
    const mod = require(nodePath);
    const StampdDb = mod.StampdDb;
    if (!StampdDb) {
      console.error("[rust-bridge] StampdDb not found in native addon");
      return false;
    }

    db = StampdDb.open(dbPath);
    console.log("[rust-bridge] Rust bridge initialized");
    return true;
  } catch (err) {
    console.error("[rust-bridge] Failed to initialize:", err);
    return false;
  }
}

/**
 * Get queue statistics.
 */
export function queueStats(): { pending: number; delivered: number; dead: number } {
  if (!db) {
    return { pending: 0, delivered: 0, dead: 0 };
  }
  return db.queueStats();
}

/**
 * Get server configuration.
 */
export function getConfig(): { domain: string; signupEnabled: boolean; dkimSelector: string } {
  if (!db) {
    return { domain: "localhost", signupEnabled: true, dkimSelector: "default" };
  }
  return db.getConfig();
}

/**
 * Check if a domain is allowed.
 */
export function isDomainAllowed(domain: string): boolean {
  if (!db) {
    return false;
  }
  return db.isDomainAllowed(domain);
}

/**
 * Get domain owner email.
 */
export function getDomainOwner(domain: string): string | null {
  if (!db) {
    return null;
  }
  return db.getDomainOwner(domain) || null;
}

/**
 * List custom domains for a user.
 */
export function listCustomDomains(userId: number): any[] {
  if (!db) {
    return [];
  }
  return db.listCustomDomains(userId);
}

/**
 * Get user by ID.
 */
export function getUser(userId: number): any {
  if (!db) {
    return null;
  }
  return db.getUser(userId) || null;
}
