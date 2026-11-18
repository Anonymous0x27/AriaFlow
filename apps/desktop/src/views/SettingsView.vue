<script setup lang="ts">
import { onMounted, ref, reactive, computed } from "vue";
import {
  api, apiBase, apiToken, setApiBase, setApiToken,
  transportMode, setTransportMode, ipcPath, setIpcPath,
  daemonBinaryPath, setDaemonBinaryPath, isTauriEnv,
  probeDaemon, ensureDaemon, findDaemonBinary,
  type DaemonCheck, type GlobalSettings, type ProxyEntry, type ProxyInput, type TransportMode,
  type SshCertificateEntry, type SshCertificateInput,
} from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { Locale } from "../lib/messages";
import DirPickerDialog from "../components/DirPickerDialog.vue";
import AriaSelect from "../components/AriaSelect.vue";
import AriaColorPicker from "../components/AriaColorPicker.vue";
import { useDownloadsStore } from "../stores/downloads";

const { t, locale, setLocale } = useI18n();
const store = useDownloadsStore();

const dirPickerOpen = ref(false);
const folderIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true">
    <path d="M3.5 6.8A2.3 2.3 0 0 1 5.8 4.5h3.7l2 2.3h6.7a2.3 2.3 0 0 1 2.3 2.3v8.2a2.3 2.3 0 0 1-2.3 2.3H5.8a2.3 2.3 0 0 1-2.3-2.3V6.8Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
  </svg>`;

function onDirPicked(path: string) {
  settings.downloadDir = path;
  dirPickerOpen.value = false;
}

// ---------- daemon settings ----------
const settings = reactive<Partial<GlobalSettings>>({});
const loading = ref(true);
const saving = ref(false);
const message = ref<string | null>(null);
const isError = ref(false);
// ---------- managed User-Agent list ----------
const userAgents = ref<string[]>([]);
const newUserAgent = ref("");
const uaMessage = ref<string | null>(null);

function addUserAgent() {
  const v = newUserAgent.value.trim();
  if (!v) return;
  if (userAgents.value.includes(v)) {
    uaMessage.value = t("settings.userAgents.duplicate");
    return;
  }
  userAgents.value.push(v);
  newUserAgent.value = "";
  uaMessage.value = null;
}

function removeUserAgent(index: number) {
  userAgents.value.splice(index, 1);
  uaMessage.value = null;
}

async function loadSettings() {
  loading.value = true;
  try {
    Object.assign(settings, await api.settings());
    userAgents.value = [...(settings.userAgents ?? [])];
    setMessage(null);
    syncWindowForm();
    syncHookForm();
    syncApiBind();
  } catch (err) {
    setMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    loading.value = false;
  }
}

async function saveSettings() {
  saving.value = true;
  try {
    const updated = await api.updateSettings({
      downloadDir: settings.downloadDir,
      maxConcurrentTasks: num(settings.maxConcurrentTasks, 5),
      maxConnectionsPerTask: num(settings.maxConnectionsPerTask, 16),
      maxTotalConnections: num(settings.maxTotalConnections, 64),
      globalSpeedLimit: num(settings.globalSpeedLimit, 0),
      perTaskSpeedLimit: num(settings.perTaskSpeedLimit, 0),
      retryCount: num(settings.retryCount, 5),
      userAgent: settings.userAgent,
      userAgents: [...userAgents.value],
      deleteFilesOnRemove: Boolean(settings.deleteFilesOnRemove),
    });
    Object.assign(settings, updated);
    userAgents.value = [...(updated.userAgents ?? [])];
    store.deleteFilesOnRemove = updated.deleteFilesOnRemove;
    setMessage(t("settings.download.saved"));
  } catch (err) {
    setMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    saving.value = false;
  }
}

function num(v: number | undefined, fallback: number): number {
  const n = Number(v);
  return Number.isFinite(n) ? n : fallback;
}

function setMessage(text: string | null, error = false) {
  message.value = text;
  isError.value = error;
}

// ---------- daily window ----------
function minuteToHHMM(minute: number): string {
  const h = Math.floor(minute / 60);
  const m = minute % 60;
  return `${String(h).padStart(2, "0")}:${String(m).padStart(2, "0")}`;
}

function parseHHMM(value: string): number | null {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value.trim());
  if (!match) return null;
  const h = Number(match[1]);
  const m = Number(match[2]);
  if (h >= 24 || m >= 60) return null;
  return h * 60 + m;
}

const windowEnabled = ref(false);
const windowStart = ref("02:00");
const windowEnd = ref("06:00");
const windowSaving = ref(false);
const windowMessage = ref<string | null>(null);
const windowIsError = ref(false);

// ---------- post-download hooks ----------
const hookComplete = ref("");
const hookError = ref("");
const hookSaving = ref(false);
const hookMessage = ref<string | null>(null);
const hookIsError = ref(false);

function syncHookForm() {
  hookComplete.value = settings.onDownloadCompleteCommand ?? "";
  hookError.value = settings.onDownloadErrorCommand ?? "";
}

function setHookMessage(text: string | null, error = false) {
  hookMessage.value = text;
  hookIsError.value = error;
}

async function saveHooks() {
  hookSaving.value = true;
  try {
    const updated = await api.updateSettings({
      onDownloadCompleteCommand: hookComplete.value.trim(),
      onDownloadErrorCommand: hookError.value.trim(),
    });
    Object.assign(settings, updated);
    setHookMessage(t("settings.hooks.saved"));
  } catch (err) {
    setHookMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    hookSaving.value = false;
  }
}

// ---------- auto quit daemon (desktop tray) ----------
const autoQuitSaving = ref(false);
const autoQuitMessage = ref<string | null>(null);
const autoQuitIsError = ref(false);

async function toggleAutoQuit(event: Event) {
  const checked = (event.target as HTMLInputElement).checked;
  autoQuitSaving.value = true;
  try {
    const updated = await api.updateSettings({ autoQuitDaemon: checked });
    Object.assign(settings, updated);
    autoQuitMessage.value = t("settings.features.saved");
    autoQuitIsError.value = false;
  } catch (err) {
    (event.target as HTMLInputElement).checked = !checked;
    autoQuitMessage.value = err instanceof Error ? err.message : String(err);
    autoQuitIsError.value = true;
  } finally {
    autoQuitSaving.value = false;
  }
}

// ---------- api-bind (boot-time listener) ----------
const apiBindValue = ref("");
const apiBindSaving = ref(false);
const apiBindMessage = ref<string | null>(null);
const apiBindIsError = ref(false);

const apiBindInvalid = computed(() => {
  const v = apiBindValue.value.trim();
  const m = /^(\[[0-9a-fA-F:.]+\]|[0-9a-fA-F.]+|[A-Za-z0-9][A-Za-z0-9.-]*):(\d{1,5})$/.exec(v);
  if (!m) return true;
  const port = Number(m[2]);
  return port < 1 || port > 65535;
});

const apiBindNonLoopback = computed(() => {
  const v = apiBindValue.value.trim();
  const m = /^(?:\[([0-9a-fA-F:.]+)\]|([0-9a-fA-F.]+)|([A-Za-z0-9][A-Za-z0-9.-]*)):(\d{1,5})$/.exec(v);
  if (!m) return false;
  const ip = (m[1] ?? m[2] ?? m[3] ?? "").toLowerCase();
  return !(ip === "localhost" || ip === "::1" || ip.startsWith("127."));
});

function syncApiBind() {
  apiBindValue.value = settings.apiBind ?? "";
}

async function saveApiBind() {
  if (apiBindInvalid.value) {
    apiBindMessage.value = t("settings.bind.invalid");
    apiBindIsError.value = true;
    return;
  }
  apiBindSaving.value = true;
  try {
    const updated = await api.updateSettings({ apiBind: apiBindValue.value.trim() });
    Object.assign(settings, updated);
    syncApiBind();
    apiBindMessage.value = t("settings.bind.saved");
    apiBindIsError.value = false;
  } catch (err) {
    apiBindMessage.value = err instanceof Error ? err.message : String(err);
    apiBindIsError.value = true;
  } finally {
    apiBindSaving.value = false;
  }
}

// ---------- live feature switches ----------
const featuresSaving = ref<string | null>(null);
const featuresMessage = ref<string | null>(null);
const featuresIsError = ref(false);

async function toggleFeature(
  key: "apiEnabled" | "mcpEnabled" | "webUiEnabled",
  event: Event,
) {
  const checked = (event.target as HTMLInputElement).checked;
  featuresSaving.value = key;
  try {
    const updated = await api.updateSettings({ [key]: checked });
    Object.assign(settings, updated);
    featuresMessage.value = t("settings.features.saved");
    featuresIsError.value = false;
  } catch (err) {
    // Roll the checkbox back to the last persisted state.
    const target = event.target as HTMLInputElement;
    target.checked = !checked;
    featuresMessage.value = err instanceof Error ? err.message : String(err);
    featuresIsError.value = true;
  } finally {
    featuresSaving.value = null;
  }
}

function syncWindowForm() {
  const start = settings.downloadWindowStartMinute ?? null;
  const end = settings.downloadWindowEndMinute ?? null;
  windowEnabled.value = start !== null && end !== null;
  if (start !== null) windowStart.value = minuteToHHMM(start);
  if (end !== null) windowEnd.value = minuteToHHMM(end);
}

function setWindowMessage(text: string | null, error = false) {
  windowMessage.value = text;
  windowIsError.value = error;
}

async function saveWindow() {
  const start = parseHHMM(windowStart.value);
  const end = parseHHMM(windowEnd.value);
  if (start === null || end === null || start === end) {
    setWindowMessage(t("settings.window.invalid"), true);
    return;
  }
  windowSaving.value = true;
  try {
    const updated = await api.updateSettings({
      downloadWindowStartMinute: start,
      downloadWindowEndMinute: end,
    });
    Object.assign(settings, updated);
    setWindowMessage(t("settings.window.saved"));
  } catch (err) {
    setWindowMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    windowSaving.value = false;
  }
}

async function clearWindow() {
  windowSaving.value = true;
  try {
    const updated = await api.updateSettings({
      downloadWindowStartMinute: null,
      downloadWindowEndMinute: null,
    });
    Object.assign(settings, updated);
    windowEnabled.value = false;
    setWindowMessage(t("settings.window.cleared"));
  } catch (err) {
    setWindowMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    windowSaving.value = false;
  }
}

// ---------- proxies ----------
const proxies = ref<ProxyEntry[]>([]);
const proxiesLoading = ref(false);
const proxyMessage = ref<string | null>(null);
const proxyIsError = ref(false);
const editingId = ref<string | null>(null);
const proxyForm = reactive<ProxyInput>({
  name: "",
  kind: "http",
  address: "",
  username: null,
  password: null,
  bypass: null,
  isDefault: false,
});
const showProxyForm = ref(false);
const proxyKindOptions = [
  { value: "http", label: "http" },
  { value: "https", label: "https" },
  { value: "socks5", label: "socks5" },
  { value: "socks5h", label: "socks5h" },
];

async function loadProxies() {
  proxiesLoading.value = true;
  try {
    proxies.value = await api.listProxies();
  } catch (err) {
    setProxyMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    proxiesLoading.value = false;
  }
}

function setProxyMessage(text: string | null, error = false) {
  proxyMessage.value = text;
  proxyIsError.value = error;
}

function resetProxyForm() {
  editingId.value = null;
  proxyForm.name = "";
  proxyForm.kind = "http";
  proxyForm.address = "";
  proxyForm.username = null;
  proxyForm.password = null;
  proxyForm.bypass = null;
  proxyForm.isDefault = false;
}

function startAddProxy() {
  resetProxyForm();
  showProxyForm.value = true;
}

function startEditProxy(entry: ProxyEntry) {
  editingId.value = entry.id;
  proxyForm.name = entry.name;
  proxyForm.kind = entry.kind;
  proxyForm.address = entry.address;
  proxyForm.username = entry.username;
  proxyForm.password = entry.password;
  proxyForm.bypass = entry.bypass;
  proxyForm.isDefault = entry.isDefault;
  showProxyForm.value = true;
}

function cancelProxyForm() {
  showProxyForm.value = false;
  resetProxyForm();
}

function validProxyForm(): boolean {
  const address = proxyForm.address.trim();
  return (
    proxyForm.name.trim().length > 0 &&
    /^(http|https|socks5|socks5h):\/\//.test(address) &&
    ["http", "https", "socks5", "socks5h"].includes(proxyForm.kind)
  );
}

async function submitProxyForm() {
  if (!validProxyForm()) {
    setProxyMessage(t("settings.proxies.invalid"), true);
    return;
  }
  const input: ProxyInput = {
    name: proxyForm.name.trim(),
    kind: proxyForm.kind,
    address: proxyForm.address.trim(),
    username: proxyForm.username?.trim() || null,
    password: proxyForm.password || null,
    bypass: proxyForm.bypass?.trim() || null,
    isDefault: proxyForm.isDefault,
  };
  try {
    if (editingId.value) {
      await api.updateProxy(editingId.value, input);
    } else {
      await api.createProxy(input);
    }
    setProxyMessage(t("settings.proxies.saved"));
    showProxyForm.value = false;
    resetProxyForm();
    await loadProxies();
  } catch (err) {
    setProxyMessage(err instanceof Error ? err.message : String(err), true);
  }
}

async function deleteProxy(entry: ProxyEntry) {
  try {
    await api.deleteProxy(entry.id);
    setProxyMessage(t("settings.proxies.deleted"));
    await loadProxies();
  } catch (err) {
    setProxyMessage(err instanceof Error ? err.message : String(err), true);
  }
}

async function makeProxyDefault(entry: ProxyEntry) {
  try {
    await api.updateProxy(entry.id, {
      name: entry.name,
      kind: entry.kind,
      address: entry.address,
      username: entry.username,
      password: entry.password,
      bypass: entry.bypass,
      isDefault: true,
    });
    await loadProxies();
  } catch (err) {
    setProxyMessage(err instanceof Error ? err.message : String(err), true);
  }
}

// ---------- ssh certificates (for ssh/sftp certificate auth) ----------
const certificates = ref<SshCertificateEntry[]>([]);
const certificatesLoading = ref(false);
const certMessage = ref<string | null>(null);
const certIsError = ref(false);
const editingCertId = ref<string | null>(null);
const certForm = reactive({
  name: "",
  privateKey: "",
  passphrase: "",
});
const showCertForm = ref(false);

async function loadCertificates() {
  certificatesLoading.value = true;
  try {
    certificates.value = await api.listSshCertificates();
  } catch (err) {
    setCertMessage(err instanceof Error ? err.message : String(err), true);
  } finally {
    certificatesLoading.value = false;
  }
}

function setCertMessage(text: string | null, error = false) {
  certMessage.value = text;
  certIsError.value = error;
}

function resetCertForm() {
  editingCertId.value = null;
  certForm.name = "";
  certForm.privateKey = "";
  certForm.passphrase = "";
}

function startAddCertificate() {
  resetCertForm();
  showCertForm.value = true;
}

function startEditCertificate(entry: SshCertificateEntry) {
  editingCertId.value = entry.id;
  certForm.name = entry.name;
  certForm.privateKey = ""; // never round-tripped; blank = keep stored key
  certForm.passphrase = "";
  showCertForm.value = true;
}

function cancelCertForm() {
  showCertForm.value = false;
  resetCertForm();
}

function validCertForm(): boolean {
  if (certForm.name.trim().length === 0) return false;
  // New certificates require the key; edits may keep the stored one.
  if (!editingCertId.value && certForm.privateKey.trim().length === 0) return false;
  if (certForm.privateKey.trim() && !certForm.privateKey.includes("PRIVATE KEY")) return false;
  return true;
}

async function submitCertForm() {
  if (!validCertForm()) {
    setCertMessage(t("settings.certificates.invalid"), true);
    return;
  }
  const input: SshCertificateInput = { name: certForm.name.trim() };
  const key = certForm.privateKey.trim();
  if (key) input.privateKey = key;
  // Blank passphrase = keep the stored one (on create: the key has none).
  if (certForm.passphrase !== "") input.passphrase = certForm.passphrase;
  try {
    if (editingCertId.value) {
      await api.updateSshCertificate(editingCertId.value, input);
    } else {
      await api.createSshCertificate(input);
    }
    setCertMessage(t("settings.certificates.saved"));
    showCertForm.value = false;
    resetCertForm();
    await loadCertificates();
  } catch (err) {
    setCertMessage(err instanceof Error ? err.message : String(err), true);
  }
}

async function deleteCertificate(entry: SshCertificateEntry) {
  try {
    await api.deleteSshCertificate(entry.id);
    setCertMessage(t("settings.certificates.deleted"));
    await loadCertificates();
  } catch (err) {
    setCertMessage(err instanceof Error ? err.message : String(err), true);
  }
}

// ---------- categories export ----------
const categoriesMessage = ref<string | null>(null);

async function exportCategories() {
  try {
    const rules = await api.categories();
    const blob = new Blob([JSON.stringify(rules, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "ariaflow-category-rules.json";
    a.click();
    URL.revokeObjectURL(url);
    categoriesMessage.value = t("settings.categories.exported");
  } catch (err) {
    categoriesMessage.value = err instanceof Error ? err.message : String(err);
  }
}

// ---------- daemon (local pipe transport + auto-start) ----------
const inTauri = isTauriEnv();
const transport = ref<TransportMode>(transportMode());
const daemonPath = ref(daemonBinaryPath());
const ipcPathValue = ref(ipcPath());
const daemonChecking = ref(false);
const daemonStarting = ref(false);
const daemonStatus = ref<DaemonCheck | null>(null);

const transportOptions = computed(() => [
  { value: "auto" as const, label: t("settings.daemon.transportAuto") },
  { value: "ipc" as const, label: t("settings.daemon.transportIpc") },
  { value: "webapi" as const, label: t("settings.daemon.transportWebapi") },
]);

function switchTransport(mode: TransportMode) {
  setTransportMode(mode);
  transport.value = mode;
}

function saveDaemonPath() {
  setDaemonBinaryPath(daemonPath.value);
}

function saveIpcPath() {
  setIpcPath(ipcPathValue.value);
}

const daemonStatusText = computed(() => {
  const s = daemonStatus.value;
  if (!s) return null;
  switch (s.state) {
    case "running":
      return t("settings.daemon.status.running", { version: s.version ? ` · v${s.version}` : "" });
    case "started":
      return t("settings.daemon.status.started", { detail: s.detail });
    case "tcp-only":
      return t("settings.daemon.status.tcpOnly");
    case "not-running":
      return t("settings.daemon.status.notRunning");
    case "not-found":
      return t("settings.daemon.status.notFound", { error: s.detail });
    case "found":
      return s.detail;
    case "web-only":
      return t("settings.daemon.webOnlyNote");
    default:
      return t("settings.daemon.status.error", { error: s.detail });
  }
});

const daemonStatusTone = computed(() => {
  const s = daemonStatus.value?.state;
  if (s === "running" || s === "started" || s === "found") return "ok";
  if (s === "not-running" || s === "tcp-only") return "warn";
  if (s === "web-only") return "dim";
  return "bad";
});

async function probeDaemonStatus() {
  daemonChecking.value = true;
  try {
    daemonStatus.value = await probeDaemon();
  } catch (err) {
    daemonStatus.value = {
      state: "error",
      endpoint: "",
      detail: err instanceof Error ? err.message : String(err),
      version: null,
    };
  } finally {
    daemonChecking.value = false;
  }
}

async function startDaemonNow() {
  daemonStarting.value = true;
  try {
    daemonStatus.value = await ensureDaemon();
  } catch (err) {
    daemonStatus.value = {
      state: "start-failed",
      endpoint: "",
      detail: err instanceof Error ? err.message : String(err),
      version: null,
    };
  } finally {
    daemonStarting.value = false;
  }
}

async function autofillDaemonPath() {
  const found = await findDaemonBinary();
  if (found) {
    daemonPath.value = found;
    saveDaemonPath();
    daemonStatus.value = {
      state: "found",
      endpoint: "",
      detail: t("settings.daemon.detectOk", { path: found }),
      version: null,
    };
  } else {
    daemonStatus.value = {
      state: "not-found",
      endpoint: "",
      detail: t("settings.daemon.detectFail"),
      version: null,
    };
  }
}

// ---------- connection settings ----------
const base = ref(apiBase());
const token = ref(apiToken());
const testResult = ref<string | null>(null);

async function testConnection() {
  setApiBase(base.value.trim());
  setApiToken(token.value.trim());
  testResult.value = t("settings.connection.testing");
  try {
    const status = await api.status();
    testResult.value = t("settings.connection.ok", { version: status.version, uptime: status.uptimeSecs });
  } catch (err) {
    testResult.value = t("settings.connection.fail", {
      error: err instanceof Error ? err.message : String(err),
    });
  }
}

// ---------- appearance ----------
const ACCENTS = computed(() => [
  { nameKey: "accent.ocean", value: "#0a84ff" },
  { nameKey: "accent.bamboo", value: "#34c759" },
  { nameKey: "accent.sun", value: "#ff9f0a" },
  { nameKey: "accent.pomegranate", value: "#ff375f" },
  { nameKey: "accent.elegant", value: "#bf5af2" },
  { nameKey: "accent.jade", value: "#64d2ff" },
]);

const APPEARANCE_KEY = "ariaflow.appearance";

interface Appearance {
  theme: "auto" | "light" | "dark";
  accent: string;
  radius: number;
  /** Whole-UI zoom factor (1 = 100%). Applied via `documentElement.style.zoom`. */
  fontScale: number;
}

/** Accepts #rgb / #rrggbb (any case) → normalised lowercase #rrggbb; otherwise null. */
function normalizeHex(input: string): string | null {
  const raw = input.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{3}$/.test(raw)) {
    return ("#" + raw.split("").map((c) => c + c).join("")).toLowerCase();
  }
  if (/^[0-9a-fA-F]{6}$/.test(raw)) {
    return ("#" + raw).toLowerCase();
  }
  return null;
}

const FONT_SCALES = [
  { key: "settings.appearance.fontScaleSmall", value: 0.9 },
  { key: "settings.appearance.fontScaleNormal", value: 1 },
  { key: "settings.appearance.fontScaleLarge", value: 1.15 },
  { key: "settings.appearance.fontScaleXLarge", value: 1.3 },
] as const;

function clampFontScale(value: unknown): number {
  const n = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(n)) return 1;
  return Math.min(1.5, Math.max(0.75, n));
}

function loadAppearance(): Appearance {
  try {
    const raw = localStorage.getItem(APPEARANCE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<Appearance>;
      return {
        theme: parsed.theme === "light" || parsed.theme === "dark" ? parsed.theme : "auto",
        accent: normalizeHex(String(parsed.accent ?? "")) ?? "#0a84ff",
        radius: typeof parsed.radius === "number" ? parsed.radius : 12,
        fontScale: clampFontScale(parsed.fontScale),
      };
    }
  } catch {
    // ignore corrupted preferences
  }
  return { theme: "auto", accent: "#0a84ff", radius: 12, fontScale: 1 };
}

const appearance = reactive<Appearance>(loadAppearance());
const accentDraft = ref(appearance.accent);
const accentInvalid = ref(false);

/** Value handed to the custom picker; must always be a valid #rrggbb. */
const accentForPicker = computed(() => normalizeHex(appearance.accent) ?? "#0a84ff");

const isPresetAccent = computed(() =>
  ACCENTS.value.some((c) => c.value.toLowerCase() === appearance.accent.toLowerCase()),
);

function applyAppearance() {
  const root = document.documentElement;
  root.setAttribute("data-theme", appearance.theme);
  root.style.setProperty("--af-accent", appearance.accent);
  root.style.setProperty("--af-accent-soft", appearance.accent + "21");
  root.style.setProperty("--af-radius", `${appearance.radius}px`);
  root.style.zoom = String(appearance.fontScale);
  localStorage.setItem(APPEARANCE_KEY, JSON.stringify(appearance));
}

function onCustomAccent(value: string) {
  const normalized = normalizeHex(value);
  if (normalized) {
    appearance.accent = normalized;
    accentDraft.value = normalized;
    accentInvalid.value = false;
    applyAppearance();
  }
}

function commitAccentDraft() {
  const normalized = normalizeHex(accentDraft.value);
  if (normalized) {
    appearance.accent = normalized;
    accentDraft.value = normalized;
    accentInvalid.value = false;
    applyAppearance();
  } else {
    accentInvalid.value = true;
    accentDraft.value = appearance.accent;
  }
}

function setFontScale(value: number) {
  appearance.fontScale = value;
  applyAppearance();
}

function switchLanguage(lang: Locale) {
  setLocale(lang);
}

onMounted(() => {
  applyAppearance();
  void loadSettings();
  void loadProxies();
  void loadCertificates();
  if (inTauri) void probeDaemonStatus();
});
</script>

<template>
  <div class="settings-view">
    <!-- language -->
    <section class="af-panel card">
      <h2>{{ t("settings.language.title") }}</h2>
      <div class="form-grid">
        <div class="appearance-block">
          <div class="block-label">{{ t("settings.language.label") }}</div>
          <div class="segmented">
            <button
              v-for="opt in (['zh-CN', 'en'] as const)"
              :key="opt"
              class="seg-btn"
              :class="{ active: locale.locale === opt }"
              @click="switchLanguage(opt)"
            >
              {{ opt === "zh-CN" ? t("settings.language.zh") : t("settings.language.en") }}
            </button>
          </div>
        </div>
      </div>
    </section>

    <!-- daemon: pipe transport + auto-start -->
    <section class="af-panel card">
      <h2>{{ t("settings.daemon.title") }}</h2>
      <p class="hint">{{ t("settings.daemon.hint") }}</p>
      <div class="segmented">
        <button
          v-for="opt in transportOptions"
          :key="opt.value"
          class="seg-btn"
          :class="{ active: transport === opt.value }"
          @click="switchTransport(opt.value)"
        >
          {{ opt.label }}
        </button>
      </div>

      <div v-if="transport !== 'webapi'" class="form-grid single">
        <div class="field">
          <label class="field-label">{{ t("settings.daemon.socketPath") }}</label>
          <input
            v-model="ipcPathValue"
            class="af-input mono"
            type="text"
            spellcheck="false"
            :placeholder="t('settings.daemon.socketPathPlaceholder')"
            @change="saveIpcPath"
          />
          <p v-if="!inTauri" class="hint">{{ t("settings.daemon.ipcBrowserNote") }}</p>
        </div>
      </div>

      <template v-if="inTauri">
        <div class="form-grid single">
          <div class="field">
            <label class="field-label">{{ t("settings.daemon.binaryPath") }}</label>
            <div class="daemon-bin-row">
              <input
                v-model="daemonPath"
                class="af-input mono"
                type="text"
                spellcheck="false"
                :placeholder="t('settings.daemon.binaryPathPlaceholder')"
                @change="saveDaemonPath"
              />
              <button class="af-btn" @click="autofillDaemonPath">{{ t("settings.daemon.detect") }}</button>
            </div>
          </div>
        </div>
        <div class="daemon-actions">
          <button class="af-btn" :disabled="daemonChecking" @click="probeDaemonStatus">
            {{ daemonChecking ? t("settings.daemon.testing") : t("settings.daemon.test") }}
          </button>
          <button class="af-btn af-btn-primary" :disabled="daemonStarting" @click="startDaemonNow">
            {{ daemonStarting ? t("settings.daemon.starting") : t("settings.daemon.start") }}
          </button>
        </div>
        <p v-if="daemonStatusText" class="daemon-status" :data-tone="daemonStatusTone">{{ daemonStatusText }}</p>
        <p class="hint">{{ t("settings.daemon.note") }}</p>
        <div class="feature-toggle-list auto-quit-list">
          <label class="feature-toggle">
            <input
              type="checkbox"
              :checked="settings.autoQuitDaemon ?? false"
              :disabled="autoQuitSaving"
              @change="toggleAutoQuit"
            />
            <span>{{ t("settings.daemon.autoQuit.label") }}</span>
          </label>
        </div>
        <p class="hint">{{ t("settings.daemon.autoQuit.hint") }}</p>
        <p v-if="autoQuitMessage" class="hint" :class="{ error: autoQuitIsError }">{{ autoQuitMessage }}</p>
      </template>
      <p v-else class="hint">{{ t("settings.daemon.webOnlyNote") }}</p>
    </section>

    <!-- api-bind: boot-time listener (item: no UI existed for this) -->
    <section class="af-panel card">
      <h2>{{ t("settings.bind.title") }}</h2>
      <p class="hint">{{ t("settings.bind.hint") }}</p>
      <div class="form-grid single">
        <label>{{ t("settings.bind.label") }}
          <input
            v-model="apiBindValue"
            class="af-input mono"
            type="text"
            spellcheck="false"
            autocomplete="off"
            placeholder="127.0.0.1:8787"
            @keydown.enter="saveApiBind"
          />
        </label>
      </div>
      <p v-if="apiBindNonLoopback" class="warn-box">{{ t("settings.bind.nonLoopback") }}</p>
      <div class="bind-actions">
        <button class="af-btn af-btn-primary" :disabled="apiBindSaving" @click="saveApiBind">
          {{ apiBindSaving ? t("settings.bind.saving") : t("settings.bind.save") }}
        </button>
        <span v-if="apiBindMessage" class="hint" :class="{ error: apiBindIsError }">{{ apiBindMessage }}</span>
      </div>
    </section>

    <!-- live feature switches -->
    <section class="af-panel card">
      <h2>{{ t("settings.features.title") }}</h2>
      <p class="hint">{{ t("settings.features.hint") }}</p>
      <div class="feature-toggle-list">
        <label class="feature-toggle">
          <input
            type="checkbox"
            :checked="settings.apiEnabled ?? true"
            :disabled="featuresSaving !== null"
            @change="toggleFeature('apiEnabled', $event)"
          />
          <span>{{ t("settings.features.api") }}</span>
        </label>
        <label class="feature-toggle">
          <input
            type="checkbox"
            :checked="settings.mcpEnabled ?? false"
            :disabled="featuresSaving !== null"
            @change="toggleFeature('mcpEnabled', $event)"
          />
          <span>{{ t("settings.features.mcp") }}</span>
        </label>
        <label class="feature-toggle">
          <input
            type="checkbox"
            :checked="settings.webUiEnabled ?? true"
            :disabled="featuresSaving !== null"
            @change="toggleFeature('webUiEnabled', $event)"
          />
          <span>{{ t("settings.features.webUi") }}</span>
        </label>
      </div>
      <p v-if="featuresMessage" class="hint" :class="{ error: featuresIsError }">{{ featuresMessage }}</p>
    </section>

    <!-- connection -->
    <section class="af-panel card">
      <h2>{{ t("settings.connection.title") }}</h2>
      <p class="hint">{{ t("settings.connection.hint") }}<code>http://127.0.0.1:8787</code>。</p>
      <div class="form-grid">
        <label>{{ t("settings.connection.apiUrl") }}<input v-model="base" class="af-input" spellcheck="false" /></label>
        <label>{{ t("settings.connection.token") }}<input v-model="token" class="af-input" type="password" :placeholder="t('settings.connection.tokenPlaceholder')" autocomplete="off" /></label>
      </div>
      <div class="row">
        <button class="af-btn" @click="testConnection">{{ t("settings.connection.test") }}</button>
        <span v-if="testResult" class="hint">{{ testResult }}</span>
      </div>
    </section>

    <!-- download -->
    <section class="af-panel card">
      <h2>{{ t("settings.download.title") }}</h2>
      <p v-if="loading" class="hint">{{ t("settings.download.loading") }}</p>
      <div v-else class="form-grid">
        <label class="wide">{{ t("settings.download.dir") }}
          <span class="save-dir-row">
            <input v-model="settings.downloadDir" class="af-input mono" spellcheck="false" />
            <button class="af-btn browse-btn" type="button" :title="t('picker.browse')" :aria-label="t('picker.browse')" @click="dirPickerOpen = true">
              <span class="browse-icon" v-html="folderIcon" />{{ t("picker.browse") }}
            </button>
          </span>
        </label>
        <label>{{ t("settings.download.maxConcurrent") }}<input v-model.number="settings.maxConcurrentTasks" class="af-input" type="number" min="1" max="64" /></label>
        <label>{{ t("settings.download.connections") }}<input v-model.number="settings.maxConnectionsPerTask" class="af-input" type="number" min="1" max="64" /></label>
        <label>{{ t("settings.download.totalConnections") }}<input v-model.number="settings.maxTotalConnections" class="af-input" type="number" min="1" max="256" /></label>
        <label>{{ t("settings.download.retries") }}<input v-model.number="settings.retryCount" class="af-input" type="number" min="0" max="20" /></label>
        <label>{{ t("settings.download.globalSpeed") }}<input v-model.number="settings.globalSpeedLimit" class="af-input" type="number" min="0" /></label>
        <label>{{ t("settings.download.perTaskSpeed") }}<input v-model.number="settings.perTaskSpeedLimit" class="af-input" type="number" min="0" /></label>
        <label class="wide">{{ t("settings.download.userAgent") }}<input v-model="settings.userAgent" class="af-input mono" spellcheck="false" /></label>
        <div class="ua-manager wide">
          <span class="field-label">{{ t("settings.userAgents.title") }}</span>
          <p class="hint">{{ t("settings.userAgents.hint") }}</p>
          <ul v-if="userAgents.length" class="ua-list">
            <li
              v-for="(ua, index) in userAgents"
              :key="ua"
              class="ua-item"
              :class="{ current: ua === settings.userAgent }"
            >
              <span class="ua-text mono" :title="ua">{{ ua }}</span>
              <span v-if="ua === settings.userAgent" class="ua-badge">{{ t("settings.userAgents.isDefault") }}</span>
              <button v-else class="mini-btn" type="button" @click="settings.userAgent = ua">{{ t("settings.userAgents.useAsDefault") }}</button>
              <button class="mini-btn danger" type="button" @click="removeUserAgent(index)">{{ t("settings.userAgents.remove") }}</button>
            </li>
          </ul>
          <p v-else class="hint">{{ t("settings.userAgents.empty") }}</p>
          <form class="ua-add" @submit.prevent="addUserAgent">
            <input
              v-model="newUserAgent"
              class="af-input mono"
              type="text"
              spellcheck="false"
              autocomplete="off"
              :placeholder="t('settings.userAgents.placeholder')"
            />
            <button class="af-btn" type="submit" :disabled="!newUserAgent.trim()">{{ t("settings.userAgents.add") }}</button>
          </form>
          <p v-if="uaMessage" class="hint error">{{ uaMessage }}</p>
        </div>
        <label class="check-row wide" :title="t('settings.download.deleteFilesHint')">
          <input v-model="settings.deleteFilesOnRemove" type="checkbox" />
          <span>{{ t("settings.download.deleteFiles") }}</span>
        </label>
      </div>
      <p class="hint">{{ t("settings.download.deleteFilesHint") }}</p>
      <div class="row">
        <button class="af-btn af-btn-primary" :disabled="saving" @click="saveSettings">{{ saving ? t("settings.download.saving") : t("settings.download.save") }}</button>
        <span v-if="message" class="hint" :class="{ error: isError }">{{ message }}</span>
      </div>
    </section>

    <!-- daily window -->
    <section class="af-panel card">
      <h2>{{ t("settings.window.title") }}</h2>
      <p class="hint">{{ t("settings.window.hint") }}</p>
      <label class="check-row">
        <input v-model="windowEnabled" type="checkbox" />
        <span>{{ t("settings.window.enabled") }}</span>
      </label>
      <div v-if="windowEnabled" class="form-grid window-grid">
        <label>{{ t("settings.window.start") }}<input v-model="windowStart" class="af-input mono" placeholder="02:00" spellcheck="false" /></label>
        <label>{{ t("settings.window.end") }}<input v-model="windowEnd" class="af-input mono" placeholder="06:00" spellcheck="false" /></label>
      </div>
      <div class="row">
        <button v-if="windowEnabled" class="af-btn af-btn-primary" :disabled="windowSaving" @click="saveWindow">
          {{ windowSaving ? t("settings.window.saving") : t("settings.window.save") }}
        </button>
        <button v-if="windowEnabled" class="af-btn" :disabled="windowSaving" @click="clearWindow">{{ t("settings.window.clear") }}</button>
        <span v-if="windowMessage" class="hint" :class="{ error: windowIsError }">{{ windowMessage }}</span>
      </div>
    </section>

    <!-- post-download hooks -->
    <section class="af-panel card">
      <h2>{{ t("settings.hooks.title") }}</h2>
      <p class="hint">{{ t("settings.hooks.hint") }}</p>
      <div class="form-grid">
        <label class="wide">{{ t("settings.hooks.complete") }}<input v-model="hookComplete" class="af-input mono" :placeholder="t('settings.hooks.placeholder')" spellcheck="false" autocomplete="off" /></label>
        <label class="wide">{{ t("settings.hooks.error") }}<input v-model="hookError" class="af-input mono" :placeholder="t('settings.hooks.placeholder')" spellcheck="false" autocomplete="off" /></label>
      </div>
      <div class="row">
        <button class="af-btn af-btn-primary" :disabled="hookSaving" @click="saveHooks">{{ hookSaving ? t("settings.hooks.saving") : t("settings.hooks.save") }}</button>
        <span v-if="hookMessage" class="hint" :class="{ error: hookIsError }">{{ hookMessage }}</span>
      </div>
    </section>

    <!-- proxies -->
    <section class="af-panel card">
      <h2>{{ t("settings.proxies.title") }}</h2>
      <p class="hint">{{ t("settings.proxies.hint") }}</p>
      <table v-if="proxies.length" class="proxy-table">
        <thead>
          <tr>
            <th>{{ t("settings.proxies.col.name") }}</th>
            <th>{{ t("settings.proxies.col.kind") }}</th>
            <th>{{ t("settings.proxies.col.address") }}</th>
            <th>{{ t("settings.proxies.col.default") }}</th>
            <th>{{ t("settings.proxies.col.actions") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="p in proxies" :key="p.id">
            <td>{{ p.name }}</td>
            <td>{{ p.kind }}</td>
            <td class="mono">{{ p.address }}</td>
            <td>{{ p.isDefault ? "✓" : "" }}</td>
            <td class="proxy-actions">
              <button class="mini-btn" @click="startEditProxy(p)">{{ t("settings.proxies.edit") }}</button>
              <button v-if="!p.isDefault" class="mini-btn" @click="makeProxyDefault(p)">{{ t("settings.proxies.setDefault") }}</button>
              <button class="mini-btn danger" @click="deleteProxy(p)">{{ t("settings.proxies.delete") }}</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else-if="!proxiesLoading" class="hint">{{ t("settings.proxies.empty") }}</p>

      <form v-if="showProxyForm" class="proxy-form" @submit.prevent="submitProxyForm">
        <div class="form-grid">
          <label>{{ t("settings.proxies.name") }}<input v-model="proxyForm.name" class="af-input" spellcheck="false" /></label>
          <label>{{ t("settings.proxies.kind") }}
            <AriaSelect v-model="proxyForm.kind" :options="proxyKindOptions" />
          </label>
          <label class="wide">{{ t("settings.proxies.address") }}<input v-model="proxyForm.address" class="af-input mono" placeholder="http://127.0.0.1:7890" spellcheck="false" /></label>
          <label>{{ t("settings.proxies.username") }}<input v-model="proxyForm.username" class="af-input" autocomplete="off" /></label>
          <label>{{ t("settings.proxies.password") }}<input v-model="proxyForm.password" class="af-input" type="password" autocomplete="off" /></label>
          <label class="wide">{{ t("settings.proxies.bypass") }}<input v-model="proxyForm.bypass" class="af-input" placeholder="*.corp.example,10.0.0.0/8" spellcheck="false" /></label>
        </div>
        <label class="check-row">
          <input v-model="proxyForm.isDefault" type="checkbox" />
          <span>{{ t("settings.proxies.isDefault") }}</span>
        </label>
        <div class="row">
          <button class="af-btn af-btn-primary" type="submit">{{ t("settings.proxies.save") }}</button>
          <button class="af-btn" type="button" @click="cancelProxyForm">{{ t("settings.proxies.cancelEdit") }}</button>
        </div>
      </form>
      <div class="row">
        <button v-if="!showProxyForm" class="af-btn" @click="startAddProxy">{{ t("settings.proxies.add") }}</button>
        <span v-if="proxyMessage" class="hint" :class="{ error: proxyIsError }">{{ proxyMessage }}</span>
      </div>
    </section>

    <!-- ssh certificates -->
    <section class="af-panel card">
      <h2>{{ t("settings.certificates.title") }}</h2>
      <p class="hint">{{ t("settings.certificates.hint") }}</p>
      <table v-if="certificates.length" class="proxy-table">
        <thead>
          <tr>
            <th>{{ t("settings.certificates.col.name") }}</th>
            <th>{{ t("settings.certificates.col.algorithm") }}</th>
            <th>{{ t("settings.certificates.col.fingerprint") }}</th>
            <th>{{ t("settings.certificates.col.passphrase") }}</th>
            <th>{{ t("settings.certificates.col.actions") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="c in certificates" :key="c.id">
            <td>{{ c.name }}</td>
            <td>{{ c.algorithm }}</td>
            <td class="mono fingerprint" :title="c.fingerprint">{{ c.fingerprint }}</td>
            <td>{{ c.hasPassphrase ? "✓" : "" }}</td>
            <td class="proxy-actions">
              <button class="mini-btn" @click="startEditCertificate(c)">{{ t("settings.certificates.edit") }}</button>
              <button class="mini-btn danger" @click="deleteCertificate(c)">{{ t("settings.certificates.delete") }}</button>
            </td>
          </tr>
        </tbody>
      </table>
      <p v-else-if="!certificatesLoading" class="hint">{{ t("settings.certificates.empty") }}</p>

      <form v-if="showCertForm" class="proxy-form" @submit.prevent="submitCertForm">
        <div class="form-grid">
          <label>{{ t("settings.certificates.name") }}<input v-model="certForm.name" class="af-input" spellcheck="false" autocomplete="off" /></label>
          <label>{{ t("settings.certificates.passphrase") }}<input v-model="certForm.passphrase" class="af-input" type="password" autocomplete="new-password" :placeholder="t('settings.certificates.passphrasePlaceholder')" /></label>
          <label class="wide">{{ t("settings.certificates.privateKey") }}
            <textarea
              v-model="certForm.privateKey"
              class="af-input mono cert-key"
              rows="6"
              spellcheck="false"
              autocomplete="off"
              :placeholder="editingCertId ? t('settings.certificates.privateKeyKeep') : t('settings.certificates.privateKeyPlaceholder')"
            ></textarea>
          </label>
        </div>
        <p class="hint">{{ editingCertId ? t("settings.certificates.editHint") : t("settings.certificates.createHint") }}</p>
        <div class="row">
          <button class="af-btn af-btn-primary" type="submit">{{ t("settings.certificates.save") }}</button>
          <button class="af-btn" type="button" @click="cancelCertForm">{{ t("settings.certificates.cancelEdit") }}</button>
        </div>
      </form>
      <div class="row">
        <button v-if="!showCertForm" class="af-btn" @click="startAddCertificate">{{ t("settings.certificates.add") }}</button>
        <span v-if="certMessage" class="hint" :class="{ error: certIsError }">{{ certMessage }}</span>
      </div>
    </section>

    <!-- categories export -->
    <section class="af-panel card">
      <h2>{{ t("settings.categories.title") }}</h2>
      <p class="hint">{{ t("settings.categories.hint") }}</p>
      <div class="row">
        <button class="af-btn" @click="exportCategories">{{ t("settings.categories.export") }}</button>
        <span v-if="categoriesMessage" class="hint">{{ categoriesMessage }}</span>
      </div>
    </section>

    <!-- appearance -->
    <section class="af-panel card">
      <h2>{{ t("settings.appearance.title") }}</h2>
      <div class="form-grid">
        <div class="appearance-block">
          <div class="block-label">{{ t("settings.appearance.theme") }}</div>
          <div class="segmented">
            <button v-for="opt in (['auto', 'light', 'dark'] as const)" :key="opt"
                    class="seg-btn" :class="{ active: appearance.theme === opt }"
                    @click="appearance.theme = opt; applyAppearance()">
              {{ opt === 'auto' ? t('settings.appearance.auto') : opt === 'light' ? t('settings.appearance.light') : t('settings.appearance.dark') }}
            </button>
          </div>
        </div>

        <div class="appearance-block">
          <div class="block-label">{{ t("settings.appearance.accent") }}</div>
          <div class="swatches">
            <button v-for="c in ACCENTS" :key="c.value" class="swatch" :title="t(c.nameKey)"
                    :style="{ background: c.value }"
                    :class="{ active: appearance.accent.toLowerCase() === c.value.toLowerCase() }"
                    @click="appearance.accent = c.value; accentDraft = c.value; accentInvalid = false; applyAppearance()" />
            <span class="swatch-divider" aria-hidden="true" />
            <AriaColorPicker :model-value="accentForPicker" :active="!isPresetAccent" :title="t('settings.appearance.customAccent')" @update:model-value="onCustomAccent" />
            <div class="hex-field" :class="{ invalid: accentInvalid }">
              <input
                v-model="accentDraft"
                class="af-input hex-input mono"
                type="text"
                spellcheck="false"
                autocomplete="off"
                maxlength="7"
                :placeholder="t('settings.appearance.hexPlaceholder')"
                :aria-label="t('settings.appearance.customAccent')"
                @change="commitAccentDraft"
                @keydown.enter.prevent="commitAccentDraft"
              />
              <span v-if="accentInvalid" class="hex-hint">{{ t("settings.appearance.accentInvalid") }}</span>
            </div>
          </div>
        </div>

        <div class="appearance-block">
          <div class="block-label">{{ t("settings.appearance.fontScale") }}</div>
          <div class="segmented">
            <button v-for="opt in FONT_SCALES" :key="opt.value"
                    class="seg-btn" :class="{ active: Math.abs(appearance.fontScale - opt.value) < 0.001 }"
                    @click="setFontScale(opt.value)">
              {{ t(opt.key) }}
            </button>
          </div>
          <p class="hint font-scale-hint">{{ t("settings.appearance.fontScaleHint") }}</p>
        </div>

        <div class="appearance-block">
          <div class="block-label">{{ t("settings.appearance.radius", { px: appearance.radius }) }}</div>
          <input v-model.number="appearance.radius" type="range" min="0" max="20" step="1"
                 class="slider" @input="applyAppearance()" />
        </div>
      </div>
      <p class="hint">{{ t("settings.appearance.hint") }}</p>
    </section>
    <DirPickerDialog
      :open="dirPickerOpen"
      :initial-path="settings.downloadDir"
      @close="dirPickerOpen = false"
      @select="onDirPicked"
    />
  </div>
</template>
<style scoped>
.settings-view {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 860px;
  min-width: 0;
}
.card {
  min-width: 0;
  padding: 18px 20px;
}
.card h2 {
  margin: 0 0 4px;
  font-size: 15px;
  font-weight: 650;
}
.hint {
  color: var(--af-text-dim);
  font-size: 12px;
  margin: 4px 0 0;
}
.hint.error {
  color: var(--af-danger);
}
code {
  font-family: var(--af-font-mono);
  background: var(--af-panel-solid);
  border: 1px solid var(--af-border);
  border-radius: 4px;
  padding: 1px 5px;
}
.form-grid {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 12px 16px;
  margin-top: 12px;
  min-width: 0;
}
.form-grid > * {
  min-width: 0;
}
.form-grid label > .af-input,
.form-grid .asel {
  width: 100%;
  min-width: 0;
  max-width: 100%;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 12px;
  color: var(--af-text-dim);
}
label.wide {
  grid-column: 1 / -1;
}
.mono {
  font-family: var(--af-font-mono);
  font-size: 12px;
}
.row {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 14px;
}
.appearance-block {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.appearance-block:first-child {
  grid-column: 1 / -1;
}
.block-label {
  font-size: 12px;
  color: var(--af-text-dim);
}
.segmented {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 2px;
  background: var(--af-panel-solid);
  border: 1px solid var(--af-border);
  border-radius: 9px;
  padding: 3px;
  width: max-content;
  max-width: 100%;
}
.seg-btn {
  border: 0;
  background: transparent;
  padding: 6px 18px;
  border-radius: 7px;
  font-size: 12px;
  cursor: pointer;
  color: var(--af-text-dim);
}
.seg-btn.active {
  background: var(--af-accent);
  color: #fff;
}
.swatches {
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}
.swatch-divider {
  width: 1px;
  height: 22px;
  background: var(--af-border);
  margin: 0 2px;
}
.hex-field {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.hex-input {
  width: 96px;
  font-size: 12px;
  padding: 6px 9px;
}
.hex-field.invalid .hex-input {
  border-color: var(--af-danger);
}
.hex-hint {
  font-size: 10px;
  color: var(--af-danger);
  max-width: 150px;
}
.font-scale-hint {
  margin-top: 6px;
}
.swatch {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 2px solid transparent;
  outline: 2px solid transparent;
  cursor: pointer;
}
.swatch.active {
  outline: 2px solid var(--af-accent);
  outline-offset: 2px;
}
.slider {
  width: 240px;
  accent-color: var(--af-accent);
}
.check-row {
  display: flex;
  flex-direction: row;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  font-size: 12px;
  color: var(--af-text-dim);
  cursor: pointer;
}
.check-row input {
  accent-color: var(--af-accent);
}
.window-grid {
  margin-top: 4px;
  max-width: 420px;
}
.proxy-table {
  width: 100%;
  border-collapse: collapse;
  margin-top: 12px;
  font-size: 12px;
}
.proxy-table th,
.proxy-table td {
  text-align: left;
  padding: 6px 8px;
  border-bottom: 1px solid var(--af-border);
}
.proxy-table th {
  color: var(--af-text-dim);
  font-weight: 600;
  font-size: 11px;
}
.cert-key {
  width: 100%;
  font-size: 10px;
  resize: vertical;
}
.proxy-table .fingerprint {
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.proxy-table .mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
.proxy-actions {
  display: flex;
  gap: 6px;
}
.mini-btn {
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text-dim);
  border-radius: 5px;
  font-size: 11px;
  padding: 3px 8px;
  cursor: pointer;
}
.mini-btn:hover {
  color: var(--af-accent);
  border-color: var(--af-accent);
}
.mini-btn.danger:hover {
  color: var(--af-danger);
  border-color: rgba(255, 59, 48, 0.4);
}
.form-grid.single {
  grid-template-columns: minmax(0, 1fr);
  margin-top: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.field-label {
  font-size: 11px;
  color: var(--af-text-dim);
  margin-top: 8px;
}
.daemon-bin-row {
  display: flex;
  gap: 8px;
}
.daemon-bin-row .af-input {
  flex: 1;
  min-width: 0;
}
.ua-manager {
  grid-column: 1 / -1;
  border: 1px solid var(--af-border);
  border-radius: 8px;
  padding: 10px 12px;
  background: color-mix(in srgb, var(--af-panel-solid) 65%, transparent);
}
.ua-manager .field-label {
  margin-top: 0;
}
.ua-list {
  list-style: none;
  margin: 8px 0 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 220px;
  overflow-y: auto;
}
.ua-item {
  display: flex;
  align-items: center;
  gap: 8px;
  border: 1px solid var(--af-border);
  border-radius: 6px;
  padding: 6px 8px;
  background: var(--af-panel-solid);
}
.ua-item.current {
  border-color: var(--af-accent);
}
.ua-text {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 11px;
}
.ua-badge {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--af-accent);
  border: 1px solid var(--af-accent);
  border-radius: 999px;
  padding: 1px 8px;
  white-space: nowrap;
}
.ua-item .mini-btn {
  flex-shrink: 0;
}
.ua-add {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
.ua-add .af-input {
  flex: 1;
  min-width: 0;
}
.daemon-actions {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
.daemon-status {
  font-size: 12px;
  margin: 10px 0 0;
  line-height: 1.5;
  word-break: break-word;
}
.daemon-status[data-tone="ok"] {
  color: var(--af-success);
}
.daemon-status[data-tone="warn"] {
  color: var(--af-warning);
}
.daemon-status[data-tone="bad"] {
  color: var(--af-danger);
}
.daemon-status[data-tone="dim"] {
  color: var(--af-text-dim);
}
.proxy-form {
  margin-top: 12px;
  padding-top: 12px;
  border-top: 1px solid var(--af-border);
}
.save-dir-row {
  display: flex;
  gap: 8px;
}
.save-dir-row .af-input {
  flex: 1;
  min-width: 0;
}
.browse-btn {
  flex-shrink: 0;
}
.browse-icon {
  display: inline-flex;
}
@media (max-width: 900px) {
  .form-grid {
    grid-template-columns: minmax(0, 1fr);
  }
}
@media (max-width: 640px) {
  .appearance-block:first-child {
    grid-column: auto;
  }
  .seg-btn {
    padding: 6px 12px;
  }
  .row {
    flex-wrap: wrap;
  }
  .ua-item {
    flex-wrap: wrap;
  }
  .ua-text {
    flex-basis: 100%;
  }
  .ua-item .mini-btn {
    white-space: normal;
  }
}
.feature-toggle-list {
  display: flex;
  flex-direction: column;
  gap: 0.75rem;
  margin: 0.5rem 0 0.25rem;
}

.feature-toggle {
  display: flex;
  flex-direction: row;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  cursor: pointer;
  font-size: 12px;
  color: var(--af-text);
}

.feature-toggle input {
  width: 1.05rem;
  height: 1.05rem;
  accent-color: var(--af-accent, #4f7cff);
  cursor: pointer;
}

.hint.error {
  color: var(--af-danger, #ff6b6b);
}

.warn-box {
  margin: 0.35rem 0 0;
  padding: 0.5rem 0.7rem;
  border-radius: 8px;
  border: 1px solid color-mix(in srgb, #ff9f0a 45%, transparent);
  background: color-mix(in srgb, #ff9f0a 12%, transparent);
  color: #ffbf47;
  font-size: 12px;
  line-height: 1.55;
}

.bind-actions {
  display: flex;
  align-items: center;
  gap: 0.75rem;
  margin-top: 0.65rem;
  flex-wrap: wrap;
}
</style>
