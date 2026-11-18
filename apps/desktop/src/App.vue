<script setup lang="ts">
import { onMounted, onUnmounted, computed, ref, watch } from "vue";
import { useRoute } from "vue-router";
import SideNav from "./components/SideNav.vue";
import AddTaskDialog from "./components/AddTaskDialog.vue";
import AppContextMenu from "./components/AppContextMenu.vue";
import { useDownloadsStore } from "./stores/downloads";
import { api, cachedAutoQuitDaemon, ensureDaemonInBackground, exitDesktopApp, humanSpeed, onQuitRequested, quitDaemon } from "./lib/api";
import { noticeState, showNotice } from "./lib/notice";
import { useI18n } from "./lib/i18n";

const { t } = useI18n();

const store = useDownloadsStore();
const route = useRoute();

// ---------- IDM-style quick add: paste / drop / clipboard ----------
const quickAddUrl = ref<string | null>(null);
// Phone-sized screens get an off-canvas navigation drawer instead of the
// fixed sidebar column; tablets get a slim icon rail (see <style>).
const drawerOpen = ref(false);
// Toolbar toast; the shared store also lets views/dialogs raise it.
const notice = computed(() => noticeState.message);

const URL_PATTERN = /(?:https?|ftp|ftps):\/\/[^\s"'<>()\[\]{}]+/gi;

function extractUrls(text: string): string[] {
  const matches = text.match(URL_PATTERN) ?? [];
  const cleaned = matches.map((u) => u.replace(/[.,;!?)]+$/, ""));
  return [...new Set(cleaned)];
}

function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || el.isContentEditable;
}

function quickAdd(text: string) {
  const urls = extractUrls(text);
  if (urls.length === 0) return false;
  quickAddUrl.value = urls[0];
  if (urls.length > 1) showNotice(t("app.multiUrlDetected", { count: urls.length }));
  return true;
}

function onPaste(event: ClipboardEvent) {
  if (isEditableTarget(event.target)) return;
  const text = event.clipboardData?.getData("text") ?? "";
  if (quickAdd(text)) event.preventDefault();
}

function onDragOver(event: DragEvent) {
  const types = event.dataTransfer ? [...event.dataTransfer.types] : [];
  if (types.includes("text/uri-list") || types.includes("text/plain")) {
    // Mark as a valid link drop target (IDM-style drag-and-drop of links).
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "copy";
  }
}

function onDrop(event: DragEvent) {
  const text =
    event.dataTransfer?.getData("text/uri-list") ||
    event.dataTransfer?.getData("text/plain") ||
    "";
  if (quickAdd(text)) event.preventDefault();
}

async function pasteFromClipboard() {
  try {
    const text = await navigator.clipboard.readText();
    if (!quickAdd(text)) showNotice(t("app.clipboardNoUrl"));
  } catch {
    showNotice(t("app.clipboardUnavailable"));
  }
}

function onQuickAdded(filename: string) {
  showNotice(t("app.quickAddAdded", { filename }));
  void store.refresh();
}

// ---------- tray "退出 AriaFlow" → honor the global autoQuitDaemon setting ----------
let unlistenQuit: (() => void) | undefined;

/** Resolve `autoQuitDaemon` fast: cached value wins; otherwise a short-timeout probe. */
async function resolveAutoQuitDaemon(): Promise<boolean> {
  const cached = cachedAutoQuitDaemon();
  if (cached !== null) return cached;
  try {
    const settings = await Promise.race([
      api.settings(),
      new Promise<never>((_, reject) => window.setTimeout(() => reject(new Error("settings fetch timed out")), 1200)),
    ]);
    return !!settings.autoQuitDaemon;
  } catch {
    // Daemon unreachable/slow — never hold the tray quit hostage to it.
    return false;
  }
}

async function handleTrayQuit() {
  // The tray "退出" flow lands here (the shell emits ariaflow-quit instead of
  // exiting directly). The shell force-exits after 5s as a watchdog.
  //
  // Latency: neither the settings probe nor the daemon shutdown may block the
  // exit. The cached `autoQuitDaemon` flag answers instantly in the common
  // case; the daemon shutdown is signalled without waiting for it to persist.
  const autoQuit = await resolveAutoQuitDaemon();
  if (autoQuit) {
    // Fire-and-forget would be racy (the app exits before the invoke lands),
    // so await the signal itself — it returns as soon as SIGTERM is sent.
    await quitDaemon(false).catch(() => undefined);
  }
  try {
    await exitDesktopApp();
  } catch {
    // Shell already gone.
  }
}

const title = computed(() => {
  const key = route.meta.titleKey as string | undefined;
  return key ? t(key) : "AriaFlow";
});
const globalSpeed = computed(() => store.counts?.globalSpeedBytesPerSec ?? 0);

watch(
  () => route.path,
  () => {
    drawerOpen.value = false;
  },
);

function onEscapeKey(event: KeyboardEvent) {
  if (event.key === "Escape") drawerOpen.value = false;
}

onMounted(() => {
  // 桌面端启动时自动拉起 ariaflowd（失败时由离线横幅提示，不打断 UI）。
  ensureDaemonInBackground();
  window.addEventListener("keydown", onEscapeKey);
  void store.loadGlobalPrefs();
  // Warm the autoQuitDaemon cache so the tray quit flow never blocks on a
  // settings fetch (api.settings() updates the localStorage cache).
  void api.settings().then(
    () => undefined,
    () => undefined,
  );
  store.startPolling(1500);
  window.addEventListener("paste", onPaste);
  window.addEventListener("dragover", onDragOver);
  window.addEventListener("drop", onDrop);
  void onQuitRequested(handleTrayQuit).then((unlisten) => {
    unlistenQuit = unlisten;
  });
});
onUnmounted(() => {
  store.stopPolling();
  window.removeEventListener("paste", onPaste);
  window.removeEventListener("dragover", onDragOver);
  window.removeEventListener("drop", onDrop);
  window.removeEventListener("keydown", onEscapeKey);
  unlistenQuit?.();
});
</script>

<template>
  <div class="app-shell" :class="{ 'drawer-open': drawerOpen }">
    <button v-if="drawerOpen" class="drawer-backdrop" :aria-label="t('app.menuToggle')" @click="drawerOpen = false" />
    <SideNav class="app-sidebar" :open="drawerOpen" @close="drawerOpen = false" />

    <main class="app-main">
      <header class="toolbar">
        <div class="toolbar-left">
          <button
            class="icon-btn menu-btn"
            :aria-expanded="drawerOpen"
            :aria-label="t('app.menuToggle')"
            :title="t('app.menuToggle')"
            @click="drawerOpen = !drawerOpen"
          >
            <svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
              <path d="M4 7h16M4 12h16M4 17h16" />
            </svg>
          </button>
          <div class="toolbar-title">{{ title }}</div>
        </div>
        <div class="toolbar-right">
          <span v-if="notice" class="notice-pill" role="status">{{ notice }}</span>
          <button class="icon-btn" :title="t('app.clipboardButtonTitle')" :aria-label="t('app.clipboardButtonTitle')" @click="pasteFromClipboard">
            <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <rect x="8" y="2" width="8" height="4" rx="1" />
              <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
              <path d="M12 11v6" />
              <path d="m9 14 3 3 3-3" />
            </svg>
          </button>
          <span class="speed-pill" :title="t('app.globalSpeed')">↓ {{ humanSpeed(globalSpeed) }}</span>
          <span class="conn-dot" :class="{ online: store.connected }" :title="store.connected ? t('app.connected', { version: store.daemonVersion }) : (store.lastError ?? t('app.disconnected'))" />
        </div>
      </header>

      <div v-if="!store.connected" class="offline-banner">
        <strong>{{ t("app.offlineTitle") }}</strong>
        <span>{{ t("app.offlineHint", { error: store.lastError ?? "" }) }}</span>
      </div>

      <section class="content scroll-y">
        <router-view v-slot="{ Component }">
          <Transition name="page" mode="out-in">
            <component :is="Component" />
          </Transition>
        </router-view>
      </section>
    </main>

    <AddTaskDialog
      :open="quickAddUrl !== null"
      :initial-url="quickAddUrl ?? ''"
      @close="quickAddUrl = null"
      @added="onQuickAdded"
    />
    <AppContextMenu />
  </div>
</template>

<style scoped>
.app-shell {
  display: grid;
  grid-template-columns: 232px minmax(0, 1fr);
  grid-template-rows: minmax(0, 1fr);
  height: 100%;
  background: var(--af-bg);
}

.toolbar-left {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.menu-btn {
  display: none;
  border: 1px solid var(--af-border);
  border-radius: var(--af-radius-sm);
}

.drawer-backdrop {
  display: none;
}

/* Tablet: slim icon rail */
@media (max-width: 1199px) and (min-width: 768px) {
  .app-shell {
    grid-template-columns: 76px minmax(0, 1fr);
  }
}

/* Phone: off-canvas navigation drawer */
@media (max-width: 767px) {
  .app-shell {
    grid-template-columns: minmax(0, 1fr);
  }
  .app-sidebar {
    position: fixed;
    top: 0;
    bottom: 0;
    left: 0;
    width: min(264px, 84vw);
    z-index: 95;
    transform: translateX(-105%);
    transition: transform 0.22s ease;
    border-right: 1px solid var(--af-border);
    box-shadow: var(--af-shadow);
  }
  .drawer-open .app-sidebar {
    transform: translateX(0);
  }
  .drawer-backdrop {
    display: block;
    position: fixed;
    inset: 0;
    z-index: 90;
    border: 0;
    background: rgba(0, 0, 0, 0.42);
    cursor: pointer;
  }
  .menu-btn {
    display: inline-flex;
  }
}

.app-sidebar {
  border-right: 1px solid var(--af-border);
}

.app-main {
  display: flex;
  flex-direction: column;
  min-width: 0;
  height: 100%;
}

.toolbar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 22px 10px;
  user-select: none;
}

.toolbar-title {
  font-size: 17px;
  font-weight: 650;
  letter-spacing: 0.2px;
}

.toolbar-right {
  display: flex;
  align-items: center;
  gap: 12px;
}

.speed-pill {
  font-family: var(--af-font-mono);
  font-size: 12px;
  color: var(--af-text-dim);
  border: 1px solid var(--af-border);
  border-radius: 999px;
  padding: 4px 10px;
  background: var(--af-panel);
}

.conn-dot {
  width: 12px;
  height: 12px;
  border-radius: 50%;
  background: var(--af-danger);
  box-shadow: 0 0 0 3px rgba(255, 59, 48, 0.15);
}
.conn-dot.online {
  background: var(--af-success);
  box-shadow: 0 0 0 3px rgba(52, 199, 89, 0.18);
}

.offline-banner {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin: 0 22px 10px;
  padding: 10px 14px;
  border-radius: var(--af-radius-sm);
  background: rgba(255, 159, 10, 0.14);
  border: 1px solid rgba(255, 159, 10, 0.35);
  color: var(--af-text);
  font-size: 13px;
}
.offline-banner code {
  font-family: var(--af-font-mono);
  background: var(--af-panel-solid);
  padding: 1px 6px;
  border-radius: 4px;
}

.content {
  flex: 1;
  min-height: 0; /* allow the flex child to shrink below content size so .scroll-y can kick in */
  padding: 6px 22px 22px;
}

@media (max-width: 767px) {
  .toolbar {
    padding: 10px 12px 8px;
  }
  .toolbar-title {
    font-size: 15px;
  }
  .toolbar-right {
    gap: 8px;
  }
  .content {
    padding: 4px 10px 14px;
  }
  .offline-banner {
    margin: 0 10px 8px;
    padding: 8px 10px;
    font-size: 12px;
  }
  .speed-pill {
    padding: 3px 8px;
    font-size: 11px;
  }
}

@media (max-width: 560px) {
  .speed-pill {
    display: none;
  }
}
</style>
