// AriaFlow Grabber — direct HTTP Web API integration.
// Browser-owned downloads remain running until the daemon confirms task creation.
const DEFAULTS = { apiUrl: "http://127.0.0.1:8787", token: "", interceptAll: true };

function apiBase(value) {
  const url = new URL((value || DEFAULTS.apiUrl).trim());
  if (!["http:", "https:"].includes(url.protocol) || url.username || url.password || url.search || url.hash)
    throw new Error("API URL must be an HTTP(S) URL without credentials, query or fragment");
  if (url.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname))
    throw new Error("Remote API connections require HTTPS");
  return url.href.replace(/\/$/, "");
}

async function requestApi(path, options = {}) {
  const config = await chrome.storage.local.get(DEFAULTS);
  const headers = { ...options.headers };
  if (config.token) headers.Authorization = `Bearer ${config.token}`;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 10000);
  try {
    const response = await fetch(apiBase(config.apiUrl) + path, {
      ...options, headers, signal: controller.signal, credentials: "omit", redirect: "error",
    });
    if (!response.ok) {
      const body = await response.text();
      throw new Error(`API HTTP ${response.status}: ${body.slice(0, 300)}`);
    }
    return await response.json();
  } finally { clearTimeout(timer); }
}

function isHttpUrl(value) {
  try { return ["http:", "https:"].includes(new URL(value).protocol); } catch { return false; }
}

async function addDownload(url, filename, referrer) {
  if (!isHttpUrl(url)) throw new Error("Only HTTP(S) downloads can be transferred");
  const body = { url, headers: referrer && isHttpUrl(referrer) ? [["Referer", referrer]] : [] };
  if (filename) body.filename = filename.split(/[\\/]/).pop();
  return requestApi("/api/v1/downloads", {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
  });
}

function reportError(context, error) {
  console.error(`[ariaflow] ${context}:`, error);
  // A visible failure is important for context-menu requests, which have no browser download.
  chrome.notifications.create({ type: "basic", iconUrl: "icons/icon-128.png",
    title: "AriaFlow Grabber", message: `${context}: ${error.message || error}`.slice(0, 400) }).catch(() => {});
}

chrome.downloads.onCreated.addListener(async (item) => {
  const config = await chrome.storage.local.get(DEFAULTS);
  if (!config.interceptAll || !isHttpUrl(item.finalUrl || item.url)) return;
  try {
    await addDownload(item.finalUrl || item.url, item.filename, item.referrer);
  } catch (err) {
    reportError("Transfer failed; browser download continues", err);
    return;
  }
  try {
    await chrome.downloads.cancel(item.id);
    await chrome.downloads.erase({ id: item.id });
  } catch (err) {
    // The browser may have completed it while the API was responding. Never erase it then.
    console.warn("[ariaflow] task created, but browser download could not be cancelled:", err);
  }
});

chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({ id: "ariaflow-download-link", title: "Download with AriaFlow", contexts: ["link"] });
});
chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId !== "ariaflow-download-link" || !info.linkUrl) return;
  try { await addDownload(info.linkUrl, undefined, tab?.url); }
  catch (err) { reportError("Could not add download", err); }
});
