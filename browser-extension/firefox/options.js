const defaults = { apiUrl: "http://127.0.0.1:8787", token: "", interceptAll: true };
const form = document.querySelector("#settings"), result = document.querySelector("#result");
const url = document.querySelector("#apiUrl"), token = document.querySelector("#token"), intercept = document.querySelector("#interceptAll");
function values() {
  const parsed = new URL(url.value.trim());
  if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash)
    throw new Error("API URL must be HTTP(S), without credentials, query or fragment");
  if (parsed.protocol === "http:" && !["localhost", "127.0.0.1", "[::1]"].includes(parsed.hostname))
    throw new Error("Remote API connections require HTTPS");
  return { apiUrl: parsed.href.replace(/\/$/, ""), token: token.value.trim(), interceptAll: intercept.checked };
}
chrome.storage.local.get(defaults).then(config => { url.value = config.apiUrl; token.value = config.token; intercept.checked = config.interceptAll; });
form.addEventListener("submit", async event => {
  event.preventDefault();
  try { await chrome.storage.local.set(values()); result.textContent = "已保存 / Saved"; }
  catch (error) { result.textContent = String(error); }
});
document.querySelector("#test").addEventListener("click", async () => {
  result.textContent = "正在测试 / Testing…";
  let timer;
  try {
    const config = values(), headers = {};
    if (config.token) headers.Authorization = `Bearer ${config.token}`;
    const controller = new AbortController(); timer = setTimeout(() => controller.abort(), 10000);
    const response = await fetch(config.apiUrl + "/api/v1/status", { headers, credentials: "omit", redirect: "error", signal: controller.signal });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    await response.json();
    result.textContent = "连接成功（请保存设置） / Connected (save settings to apply)";
  } catch (error) { result.textContent = `连接失败 / Connection failed: ${error}`; }
  finally { clearTimeout(timer); }
});
