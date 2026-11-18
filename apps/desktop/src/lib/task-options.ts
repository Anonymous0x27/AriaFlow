/**
 * Shared builders for "advanced" download options used by the single-add and
 * batch-add dialogs (IDM-style). Pure functions — no Vue dependency.
 */
import type { AuthConfigInput, ProxyEntry, TaskProxyConfig } from "./api";

export interface AdvancedOptions {
  /** Empty = daemon's configured download directory. */
  saveDir: string;
  /** One "Key: Value" pair per line; blank lines ignored. */
  headersText: string;
  authType: "none" | "basic" | "bearer" | "certificate";
  username: string;
  password: string;
  token: string;
  /** Selected SSH certificate id when authType === "certificate"; "" = none picked yet. */
  certificateId: string;
  /** "" = leave unset (direct; do not reference named proxies), otherwise a ProxyEntry id. */
  proxyId: string;
  startPaused: boolean;
  /** "" = start ASAP, else "YYYY-MM-DDTHH:MM" local wall time (AriaDateTimePicker; same contract as datetime-local). Sent as RFC 3339. */
  scheduledStart: string;
  /** Free-form KB/s; empty/"0" = no cap. Sent as bytes/sec in limitRateBytesPerSec.
   *  Typed as string but the number-typed input may hand us a real number — always coerce. */
  limitRateText: string | number;
  /** Empty = follow the daemon's global User-Agent setting. */
  userAgent: string;
}

export const EMPTY_ADVANCED: AdvancedOptions = {
  saveDir: "",
  headersText: "",
  authType: "none",
  username: "",
  password: "",
  token: "",
  certificateId: "",
  proxyId: "",
  startPaused: false,
  scheduledStart: "",
  limitRateText: "",
  userAgent: "",
};

export interface AdvancedPayload {
  saveDir?: string;
  headers?: [string, string][];
  auth?: AuthConfigInput;
  /** Certificate auth: the id of the stored SSH certificate (ssh/sftp transfers). */
  sshCertificateId?: string;
  proxy?: TaskProxyConfig;
  startPaused?: boolean;
  scheduledStartAt?: string;
  limitRateBytesPerSec?: number;
  userAgent?: string;
}

export function parseHeaders(text: string): [string, string][] {
  const out: [string, string][] = [];
  for (const raw of text.split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    const idx = line.indexOf(":");
    if (idx <= 0) continue; // skip malformed lines rather than aborting the add
    const name = line.slice(0, idx).trim();
    const value = line.slice(idx + 1).trim();
    if (name && value) out.push([name, value]);
  }
  return out;
}

export function proxyEntryToConfig(entry: ProxyEntry): TaskProxyConfig {
  return {
    kind: entry.kind as TaskProxyConfig["kind"],
    address: entry.address,
    username: entry.username,
    password: entry.password,
    bypass: entry.bypass,
  };
}

/** Converts the dialog state into the `NewDownloadRequest` wire shape. */
export function buildAdvancedPayload(
  options: AdvancedOptions,
  proxies: ProxyEntry[],
): AdvancedPayload {
  const payload: AdvancedPayload = {};

  const dir = options.saveDir.trim();
  if (dir) payload.saveDir = dir;

  const headers = parseHeaders(options.headersText);
  if (headers.length > 0) payload.headers = headers;

  if (options.authType === "basic") {
    payload.auth = {
      type: "basic",
      username: options.username,
      password: options.password,
    };
  } else if (options.authType === "bearer") {
    payload.auth = { type: "bearer", token: options.token };
  } else if (options.authType === "certificate" && options.certificateId) {
    payload.sshCertificateId = options.certificateId;
  }

  if (options.proxyId) {
    const entry = proxies.find((p) => p.id === options.proxyId);
    if (entry) payload.proxy = proxyEntryToConfig(entry);
  }

  if (options.startPaused) payload.startPaused = true;

  const limitKb = Number.parseFloat(String(options.limitRateText ?? "").trim());
  if (Number.isFinite(limitKb) && limitKb > 0) {
    payload.limitRateBytesPerSec = Math.round(limitKb * 1024);
  }

  const userAgent = options.userAgent.trim();
  if (userAgent) payload.userAgent = userAgent;

  const scheduled = options.scheduledStart.trim();
  if (scheduled) {
    // datetime-local yields "YYYY-MM-DDTHH:MM"; interpret as local time and
    // emit RFC 3339 so the daemon can compare against UTC.
    const date = new Date(scheduled);
    if (!Number.isNaN(date.getTime())) {
      payload.scheduledStartAt = date.toISOString();
    }
  }

  return payload;
}

/**
 * Parses a newline-separated URL list into trimmed, non-empty lines.
 * Blank lines and `#` comments are skipped (same rules as `ariaflow batch-add`).
 */
export function parseUrlList(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0 && !line.startsWith("#"));
}

/**
 * Pre-submit validation for the certificate auth choice. Returns an i18n key
 * to show via toast, or null when the selection is usable.
 */
export function certificateAuthError(options: AdvancedOptions): string | null {
  if (options.authType === "certificate" && !options.certificateId) {
    return "dialog.certificateRequired";
  }
  return null;
}

/** True when `raw` parses as an ssh/sftp URL (certificate-capable transport). */
export function isSshLikeUrl(raw: string): boolean {
  try {
    const scheme = new URL(raw.trim()).protocol.replace(":", "").toLowerCase();
    return scheme === "ssh" || scheme === "sftp";
  } catch {
    return false;
  }
}

/**
 * Derives a filename from a URL's path (percent-decoded basename). Returns
 * "" when no usable segment exists (e.g. magnet/metalink manifests).
 */
export function filenameFromUrl(raw: string): string {
  try {
    const url = new URL(raw.trim());
    const segments = url.pathname.split("/").filter(Boolean);
    const last = segments.pop();
    if (!last) return "";
    try {
      return decodeURIComponent(last);
    } catch {
      return last;
    }
  } catch {
    return "";
  }
}
