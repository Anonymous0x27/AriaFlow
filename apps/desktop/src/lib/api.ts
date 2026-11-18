/**
 * HTTP client for the AriaFlow daemon (`ariaflowd`).
 *
 * The desktop app talks to the local daemon over the same REST API used by the
 * CLI — the GUI is just another client and can be closed without interrupting
 * downloads. When running inside Tauri, the base URL and token can be
 * overridden via localStorage for advanced deployments (LAN daemon etc.).
 */

export interface DownloadCounts {
  total: number;
  downloading: number;
  waiting: number;
  paused: number;
  completed: number;
  failed: number;
  cancelled: number;
  totalDownloadedBytes: number;
  globalSpeedBytesPerSec: number;
}

export interface TaskSegment {
  id: number;
  startOffset: number;
  /** Inclusive end offset, like HTTP `Content-Range`. */
  endOffset: number;
  downloadedSize: number;
  /** "pending" | "downloading" | "completed" | "failed" */
  status: string;
  retryCount: number;
}

export interface DownloadTask {
  id: string;
  url: string;
  finalUrl?: string | null;
  filename: string;
  savePath: string;
  totalSize?: number | null;
  downloadedSize: number;
  status: string;
  speedBytesPerSec: number;
  averageSpeedBytesPerSec: number;
  progress: number;
  priority: number;
  retryCount: number;
  maxConnections: number;
  currentConnections: number;
  category: string;
  etag?: string | null;
  lastModified?: string | null;
  checksum?: { algorithm: string; digest: string } | string | null;
  /** Populated by GET /api/v1/downloads/{id} only (list responses carry `[]`). */
  segments?: TaskSegment[];
  errorMessage?: string | null;
  createdAt: string;
  startedAt?: string | null;
  completedAt?: string | null;
  etaSeconds?: number | null;
  scheduledStartAt?: string | null;
  /** Per-task cap in bytes/sec; 0 = unlimited, null/undefined = inherit the global default. */
  limitRateBytesPerSec?: number | null;
}

export interface StatusResponse {
  product: string;
  version: string;
  uptimeSecs: number;
  counts: DownloadCounts;
}

export interface GlobalSettings {
  downloadDir: string;
  maxConcurrentTasks: number;
  maxConnectionsPerTask: number;
  maxTotalConnections: number;
  globalSpeedLimit: number;
  perTaskSpeedLimit: number;
  retryCount: number;
  minSegmentSize: number;
  connectTimeoutSecs: number;
  readTimeoutSecs: number;
  apiEnabled: boolean;
  apiBind: string;
  /** Show paused tasks in the overview's in-progress section (backend-persisted). */
  overviewShowPaused: boolean;
  /** Show newly created (pending/queued/waiting/retrying) tasks in the overview's in-progress section. Defaults to true. */
  overviewShowNewTasks: boolean;
  /** Stat cards hidden from the overview page; empty = show all. */
  overviewHiddenCards: string[];
  /** Default for the "also delete downloaded files" checkbox in remove confirmations. */
  deleteFilesOnRemove: boolean;
  /** Desktop-only: when the desktop app quits (tray → Quit), also shut down ariaflowd. */
  autoQuitDaemon: boolean;
  mcpEnabled: boolean;
  webUiEnabled: boolean;
  webUiBind: string;
  userAgent: string;
  /** Managed User-Agent list offered in download dialogs (seeded with common agents). */
  userAgents: string[];
  downloadWindowStartMinute: number | null;
  downloadWindowEndMinute: number | null;
  onDownloadCompleteCommand: string | null;
  onDownloadErrorCommand: string | null;
}

/** Minimal handle a “另存为…” dialog needs: scope + what to name the copy. */
export type SaveAsTarget =
  | { kind: "task"; id: string; filename: string }
  | { kind: "history"; id: number; filename: string };

export interface HistoryEntry {
  /** Row id in download_history (used for targeted deletion). */
  id: number;
  url: string;
  filename: string;
  savePath: string;
  totalSize: number | null;
  averageSpeed: number | null;
  result: "completed" | "failed" | "cancelled" | string;
  errorMessage: string | null;
  completedAt: string;
}

export interface ProxyEntry {
  id: string;
  name: string;
  kind: string;
  address: string;
  username: string | null;
  password: string | null;
  bypass: string | null;
  isDefault: boolean;
  createdAt: string | null;
}

export interface ProxyInput {
  name: string;
  kind: string;
  address: string;
  username: string | null;
  password: string | null;
  bypass: string | null;
  isDefault: boolean;
}

export type TaskProxyConfig = {
  kind: "http" | "https" | "socks5" | string;
  address: string;
  username: string | null;
  password: string | null;
  bypass: string | null;
};

export type AuthConfigInput =
  | { type: "basic"; username: string; password: string }
  | { type: "bearer"; token: string };

/** SSH certificate metadata as exposed by the daemon (never key material). */
export interface SshCertificateEntry {
  id: string;
  name: string;
  algorithm: string;
  fingerprint: string;
  hasPassphrase: boolean;
  createdAt?: string | null;
}

/**
 * Payload for creating/updating a certificate. Key material goes in only
 * here — it is never returned by any endpoint.
 * - `privateKey`: OpenSSH PEM. Required for create; blank/absent keeps the stored key.
 * - `passphrase`: absent = keep/no change; "" = clear; non-empty = set.
 */
export interface SshCertificateInput {
  name: string;
  privateKey?: string | null;
  passphrase?: string | null;
}

export interface AddTaskInput {
  url: string;
  filename?: string;
  saveDir?: string;
  headers?: [string, string][];
  auth?: AuthConfigInput;
  /** Certificate auth (ssh/sftp): id of a stored SSH certificate. */
  sshCertificateId?: string;
  proxy?: TaskProxyConfig;
  startPaused?: boolean;
  scheduledStartAt?: string;
  /** Per-task cap in bytes/sec; 0/omitted = unlimited. */
  limitRateBytesPerSec?: number;
  /** Per-task User-Agent override; omitted = follow the global setting. */
  userAgent?: string;
}

export interface BatchItemError {
  index: number;
  url: string;
  message: string;
}

export interface BatchAddResult {
  action: string;
  affected: number;
  ids?: string[];
  errors?: BatchItemError[];
}

export interface FsEntry {
  name: string;
  /** Absolute path of the entry. */
  path: string;
  /** "dir" | "file" | "other" */
  kind: string;
  /** Bytes; only for regular files. */
  size: number | null;
  /** Seconds since the Unix epoch; null when unknown. */
  modifiedSecs: number | null;
}

export interface FsListResponse {
  /** Canonical absolute path that was listed. */
  path: string;
  /** Canonical parent; null at the filesystem root. */
  parent: string | null;
  isRoot: boolean;
  showHidden: boolean;
  /** True when more entries exist than were returned (capped at 5000). */
  truncated: boolean;
  entries: FsEntry[];
}

export interface DaemonCheck {
  /** "running" | "started" | "tcp-only" | "not-running" | "not-found" | "start-failed" */
  state: string;
  endpoint: string;
  detail: string;
  version: string | null;
}

/**
 * Where the daemon lives, when nothing is configured.
 * - inside the Tauri webview: always the loopback web API (the pipe is used via IPC commands instead)
 * - vite dev/preview servers (5173/4173): the daemon is a separate process on 127.0.0.1:8787
 * - anything else (i.e. the daemon itself serves this page at `/`): same origin
 */
function defaultApiBase(): string {
  if (typeof window === "undefined") return "http://127.0.0.1:8787";
  const { port, origin } = window.location;
  if (port === "5173" || port === "4173") return "http://127.0.0.1:8787";
  if (isTauriEnv()) return "http://127.0.0.1:8787";
  if (origin.startsWith("http://") || origin.startsWith("https://")) return origin;
  return "http://127.0.0.1:8787";
}
const DEFAULT_BASE = defaultApiBase();
const BASE_KEY = "ariaflow.apiBase";
const TOKEN_KEY = "ariaflow.apiToken";
const TRANSPORT_KEY = "ariaflow.transport";
const IPC_PATH_KEY = "ariaflow.ipcPath";
const DAEMON_BIN_KEY = "ariaflow.daemonPath";

export type TransportMode = "auto" | "ipc" | "webapi";

export function transportMode(): TransportMode {
  const v = localStorage.getItem(TRANSPORT_KEY);
  return v === "ipc" || v === "webapi" || v === "auto" ? v : "auto";
}

export function setTransportMode(mode: TransportMode): void {
  localStorage.setItem(TRANSPORT_KEY, mode);
}

/** Empty string = platform default pipe (resolved by the native host). */
export function ipcPath(): string {
  return localStorage.getItem(IPC_PATH_KEY) ?? "";
}

export function setIpcPath(value: string): void {
  localStorage.setItem(IPC_PATH_KEY, value.trim());
}

export function daemonBinaryPath(): string {
  return localStorage.getItem(DAEMON_BIN_KEY) ?? "";
}

export function setDaemonBinaryPath(value: string): void {
  localStorage.setItem(DAEMON_BIN_KEY, value.trim());
}

/** True inside the Tauri webview (never in a plain browser tab). */
export function isTauriEnv(): boolean {
  return typeof window !== "undefined" && Boolean((window as unknown as Record<string, unknown>).__TAURI_INTERNALS__);
}

/** What we will actually use right now ("auto" → pipe in Tauri, web API elsewhere). */
export function effectiveTransport(): "ipc" | "webapi" {
  const mode = transportMode();
  if (mode === "webapi") return "webapi";
  if (mode === "ipc") return "ipc";
  return isTauriEnv() ? "ipc" : "webapi";
}

export function apiBase(): string {
  return localStorage.getItem(BASE_KEY) || DEFAULT_BASE;
}

export function setApiBase(value: string): void {
  localStorage.setItem(BASE_KEY, value.replace(/\/+$/, ""));
}

export function apiToken(): string {
  return localStorage.getItem(TOKEN_KEY) || "";
}

/** Cache for the `autoQuitDaemon` global setting — lets the tray "退出" flow answer instantly. */
const AUTO_QUIT_KEY = "ariaflow.autoQuitDaemon";

export function cachedAutoQuitDaemon(): boolean | null {
  try {
    const raw = localStorage.getItem(AUTO_QUIT_KEY);
    return raw === null ? null : raw === "1";
  } catch {
    return null;
  }
}

function cacheAutoQuitDaemon(value: boolean): void {
  try {
    localStorage.setItem(AUTO_QUIT_KEY, value ? "1" : "0");
  } catch {
    // storage unavailable (private mode) — cache is best-effort
  }
}

export function setApiToken(value: string): void {
  localStorage.setItem(TOKEN_KEY, value);
}

function headers(): HeadersInit {
  const h: Record<string, string> = { "Content-Type": "application/json" };
  const token = apiToken();
  if (token) h["Authorization"] = `Bearer ${token}`;
  return h;
}

interface ErrorBody {
  error?: { message?: string };
  message?: string;
}

function extractErrorMessage(parsed: unknown, fallback: string): string {
  const body = (parsed ?? {}) as ErrorBody;
  return body?.error?.message ?? body?.message ?? fallback;
}

async function webapiRequest<T>(method: string, path: string, body?: unknown): Promise<T> {
  const res = await fetch(`${apiBase()}${path}`, {
    method,
    headers: headers(),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`;
    try {
      message = extractErrorMessage(await res.json(), message);
    } catch {
      // keep the raw status message
    }
    throw new Error(message);
  }
  if (res.status === 204) return undefined as T;
  return (await res.json()) as T;
}

async function ipcInvoke<T>(cmd: string, args: Record<string, unknown>): Promise<T> {
  const mod = await import("@tauri-apps/api/core");
  return (await mod.invoke(cmd, args)) as T;
}

/**
 * Desktop-only: shut ariaflowd down via the shell (honors the
 * `autoQuitDaemon` global setting from the tray quit flow).
 * `wait: false` (tray quit) signals the daemon and returns immediately —
 * the UI can exit right away and ariaflowd shuts itself down in the
 * background. `wait: true` (default, e.g. Settings) blocks up to 3s for the
 * daemon to persist state and exit.
 */
export async function quitDaemon(wait = true): Promise<string> {
  return ipcInvoke<string>("ariaflow_quit_daemon", { wait });
}

/** Desktop-only: exit the shell. Call at the end of the tray quit flow. */
export async function exitDesktopApp(): Promise<void> {
  return ipcInvoke<void>("ariaflow_exit", {});
}

/** Start a browser download. Without a token, use the streaming attachment URL
 * immediately in the click gesture, so the browser owns the transfer and can
 * show its normal download UI. Never put bearer tokens in URLs. With a token,
 * fetch with Authorization and use a Blob because navigations cannot set it.
 * The authenticated fallback buffers the file in browser memory.
 * Desktop callers must instead use SaveAsDialog (works with IPC-only daemons).
 */
async function downloadBrowserFile(path: string, filename: string): Promise<void> {
  if (isTauriEnv()) throw new Error("Use the save dialog in the desktop app");
  const token = apiToken();
  let url = `${apiBase()}${path}`;
  let blobUrl: string | null = null;
  if (token) {
    const res = await fetch(url, { headers: { Authorization: `Bearer ${token}` } });
    if (!res.ok) {
      let message = `${res.status} ${res.statusText}`;
      try { message = extractErrorMessage(await res.json(), message); } catch { /* retain HTTP status */ }
      throw new Error(message);
    }
    blobUrl = URL.createObjectURL(await res.blob());
    url = blobUrl;
  }
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename || "ariaflow-download";
  // Keep the app open if an unauthenticated attachment endpoint returns an error.
  if (!blobUrl) anchor.target = "_blank";
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  if (blobUrl) window.setTimeout(() => URL.revokeObjectURL(blobUrl!), 60_000);
}

export function downloadTaskFile(task: { id: string; filename: string }): Promise<void> {
  return downloadBrowserFile(`/api/v1/downloads/${encodeURIComponent(task.id)}/file`, task.filename);
}

export function downloadHistoryFile(entry: { id: number; filename: string }): Promise<void> {
  return downloadBrowserFile(`/api/v1/history/${entry.id}/file`, entry.filename);
}

/**
 * Desktop-only: register a handler for the tray "Quit AriaFlow" request. The
 * shell emits `ariaflow-quit` instead of exiting directly so the UI can honor
 * the `autoQuitDaemon` global setting first. Returns an unlisten function;
 * a no-op unlisten outside Tauri.
 */
export async function onQuitRequested(
  handler: () => void | Promise<void>,
): Promise<() => void> {
  if (!isTauriEnv()) return () => {};
  try {
    const mod = await import("@tauri-apps/api/event");
    return await mod.listen("ariaflow-quit", () => {
      void handler();
    });
  } catch {
    // Never break app startup because the quit listener couldn't attach.
    return () => {};
  }
}

/** True when the IPC request never reached the daemon (safe to retry elsewhere). */
function isConnectError(err: unknown): boolean {
  return (
    typeof err === "object" &&
    err !== null &&
    (err as { kind?: string }).kind === "connect"
  );
}

async function ipcRequest<T>(method: string, path: string, body?: unknown): Promise<T> {
  let res: { status: number; body: unknown };
  try {
    res = await ipcInvoke("ariaflow_request", {
      method,
      path,
      body: body ?? null,
      token: apiToken() || null,
      ipcPath: ipcPath() || null,
    });
  } catch (err) {
    if (isConnectError(err)) {
      throw Object.assign(new Error((err as { message?: string }).message ?? "IPC connect failed"), {
        kind: "connect",
      });
    }
    throw new Error(
      typeof err === "object" && err !== null && "message" in err
        ? String((err as { message?: unknown }).message)
        : String(err),
    );
  }
  if (res.status >= 400) {
    throw new Error(extractErrorMessage(res.body, `${res.status}`));
  }
  if (res.status === 204 || res.body == null) return undefined as T;
  return res.body as T;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  if (effectiveTransport() === "ipc") {
    try {
      return await ipcRequest<T>(method, path, body);
    } catch (err) {
      // Auto mode: daemon pipe not up (yet) → graceful degradation to web API.
      if (transportMode() === "auto" && isConnectError(err)) {
        return webapiRequest<T>(method, path, body);
      }
      throw err;
    }
  }
  return webapiRequest<T>(method, path, body);
}

// ---------- daemon management (Tauri only; no-ops in the browser) ----------

export async function probeDaemon(): Promise<DaemonCheck> {
  if (!isTauriEnv()) {
    return {
      state: "web-only",
      endpoint: "",
      detail: "Pipe transport is only available inside the desktop app",
      version: null,
    };
  }
  return ipcInvoke<DaemonCheck>("ariaflow_daemon_status", {
    ipcPath: ipcPath() || null,
    webapiBase: apiBase(),
  });
}

export async function ensureDaemon(): Promise<DaemonCheck> {
  if (!isTauriEnv()) {
    return {
      state: "web-only",
      endpoint: "",
      detail: "Pipe transport is only available inside the desktop app",
      version: null,
    };
  }
  return ipcInvoke<DaemonCheck>("ariaflow_ensure_daemon", {
    binaryPath: daemonBinaryPath() || null,
    ipcPath: ipcPath() || null,
    token: apiToken() || null,
    webapiBase: apiBase(),
  });
}

export async function findDaemonBinary(): Promise<string | null> {
  if (!isTauriEnv()) return null;
  return ipcInvoke<string | null>("ariaflow_find_daemon", {});
}

/** Fire-and-forget auto-start used at app launch; failures surface via the offline banner. */
export function ensureDaemonInBackground(): void {
  if (!isTauriEnv() || effectiveTransport() === "webapi") return;
  ensureDaemon().catch((err) => console.warn("ariaflow auto-start failed:", err));
}

export const api = {
  status: () => request<StatusResponse>("GET", "/api/v1/status"),
  list: (params: Record<string, string> = {}) => {
    const qs = new URLSearchParams(params).toString();
    return request<DownloadTask[]>("GET", `/api/v1/downloads${qs ? `?${qs}` : ""}`);
  },
  get: (id: string) => request<DownloadTask>("GET", `/api/v1/downloads/${id}`),
  add: (url: string, extra: Record<string, unknown> = {}) =>
    request<DownloadTask>("POST", "/api/v1/downloads", { url, ...extra }),
  pause: (id: string) => request<void>("POST", `/api/v1/downloads/${id}/pause`),
  resume: (id: string) => request<void>("POST", `/api/v1/downloads/${id}/resume`),
  cancel: (id: string) => request<void>("POST", `/api/v1/downloads/${id}/cancel`),
  retry: (id: string) => request<void>("POST", `/api/v1/downloads/${id}/retry`),
  remove: (id: string, deleteFile = false) =>
    request<void>("DELETE", `/api/v1/downloads/${id}?deleteFile=${deleteFile}`),
  /** Server-side copy of a COMPLETED task's file into `destDir` (daemon host). */
  saveDownloadAs: (id: string, destDir: string, filename?: string, overwrite = false) =>
    request<{ savedTo: string; size: number }>(
      "POST",
      `/api/v1/downloads/${encodeURIComponent(id)}/save-as`,
      { destDir, filename: filename?.trim() ? filename.trim() : undefined, overwrite },
    ),
  /** History-scoped twin (keyed by download_history row id; works after task deletion). */
  saveHistoryAs: (historyId: number, destDir: string, filename?: string, overwrite = false) =>
    request<{ savedTo: string; size: number }>(
      "POST",
      `/api/v1/history/${historyId}/save-as`,
      { destDir, filename: filename?.trim() ? filename.trim() : undefined, overwrite },
    ),
  settings: () =>
    request<GlobalSettings>("GET", "/api/v1/settings").then((s) => {
      cacheAutoQuitDaemon(!!s.autoQuitDaemon);
      return s;
    }),
  updateSettings: (patch: Record<string, unknown>) =>
    request<GlobalSettings>("PUT", "/api/v1/settings", patch).then((s) => {
      cacheAutoQuitDaemon(!!s.autoQuitDaemon);
      return s;
    }),
  history: (limit = 100) => request<HistoryEntry[]>("GET", `/api/v1/history?limit=${limit}`),
  /** Delete specific history rows, or all of them when ids is empty/omitted. */
  historyDelete: (ids?: number[]) => {
    const qs = ids && ids.length > 0 ? `?ids=${ids.join(",")}` : "";
    return request<{ deleted: number }>("DELETE", `/api/v1/history${qs}`);
  },
  categories: () => request<Record<string, unknown>>("GET", "/api/v1/categories"),
  listProxies: () => request<ProxyEntry[]>("GET", "/api/v1/proxies"),
  listSshCertificates: () => request<SshCertificateEntry[]>("GET", "/api/v1/certificates"),
  createSshCertificate: (input: SshCertificateInput) =>
    request<SshCertificateEntry>("POST", "/api/v1/certificates", input),
  updateSshCertificate: (id: string, input: SshCertificateInput) =>
    request<SshCertificateEntry>(
      "PUT",
      `/api/v1/certificates/${encodeURIComponent(id)}`,
      input,
    ),
  deleteSshCertificate: (id: string) =>
    request<void>("DELETE", `/api/v1/certificates/${encodeURIComponent(id)}`),
  createProxy: (input: ProxyInput) => request<ProxyEntry>("POST", "/api/v1/proxies", input),
  updateProxy: (id: string, input: ProxyInput) =>
    request<ProxyEntry>("PUT", `/api/v1/proxies/${encodeURIComponent(id)}`, input),
  deleteProxy: (id: string) =>
    request<void>("DELETE", `/api/v1/proxies/${encodeURIComponent(id)}`),
  setPriority: (id: string, priority: number) =>
    request<void>("PUT", `/api/v1/downloads/${encodeURIComponent(id)}/priority`, { priority }),
  addTask: (input: AddTaskInput) => request<DownloadTask>("POST", "/api/v1/downloads", input),
  batchAdd: (tasks: AddTaskInput[]) =>
    request<BatchAddResult>("POST", "/api/v1/downloads/batch", { action: "addMany", tasks }),
  /**
   * Set or clear a per-task speed cap. `null` clears the override (global
   * `perTaskSpeedLimit` applies); `0` disables the cap explicitly. In-flight
   * transfers cycle pause→resume to apply the change.
   */
  setLimitRate: (id: string, limitRateBytesPerSec: number | null) =>
    request<{ id: string; limitRateBytesPerSec: number | null; restarted: boolean }>(
      "PUT",
      `/api/v1/downloads/${encodeURIComponent(id)}/limit-rate`,
      { limitRateBytesPerSec },
    ),
  fs: {
    /** Directory listing served by ariaflowd (drives the folder picker). */
    list: (path?: string, showHidden?: boolean) => {
      const params = new URLSearchParams();
      if (path && path.trim()) params.set("path", path.trim());
      if (showHidden) params.set("showHidden", "true");
      const qs = params.toString();
      return request<FsListResponse>("GET", `/api/v1/fs/list${qs ? `?${qs}` : ""}`);
    },
  },
};

export function humanBytes(bytes?: number | null): string {
  if (bytes === null || bytes === undefined) return "?";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
}

export function humanSpeed(bytesPerSec: number): string {
  return `${humanBytes(bytesPerSec)}/s`;
}

/** Client-side ETA (the wire format carries no server-computed eta). */
export function etaSeconds(task: DownloadTask): number | null {
  if (task.totalSize == null || task.totalSize <= 0) return null;
  const speed = task.speedBytesPerSec;
  if (!speed || speed <= 0) return null;
  return Math.floor(Math.max(0, task.totalSize - task.downloadedSize) / speed);
}

export function humanEta(secs?: number | null): string {
  if (secs === null || secs === undefined) return "--";
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}
