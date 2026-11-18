<script setup lang="ts">
/**
 * App-wide context menu. Replaces the browser menu everywhere in the UI
 * (desktop + web); offers:
 *   - task rows   → 查看详情 / 暂停|继续|开始 / 重试 / 设置限速 / 复制链接 / 删除
 *   - edit fields → 剪切 / 复制 / 粘贴 / 全选
 *   - text selection → 复制
 *   - links       → 复制链接
 * Everything else: the browser menu is still suppressed.
 */
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref } from "vue";
import { api, downloadTaskFile, isTauriEnv, apiToken, type SaveAsTarget, type DownloadTask } from "../lib/api";
import { useDownloadsStore } from "../stores/downloads";
import { useI18n } from "../lib/i18n";
import TaskDetailDialog from "./TaskDetailDialog.vue";
import SpeedLimitDialog from "./SpeedLimitDialog.vue";
import ConfirmDialog from "./ConfirmDialog.vue";
import SaveAsDialog from "./SaveAsDialog.vue";

const { t } = useI18n();
const desktop = isTauriEnv();
const store = useDownloadsStore();

type MenuKind = "task" | "edit" | "selection" | "link";

const menu = reactive<{
  open: boolean;
  x: number;
  y: number;
  kind: MenuKind;
  task: DownloadTask | null;
  editEl: HTMLInputElement | HTMLTextAreaElement | HTMLElement | null;
  linkHref: string;
  selectionText: string;
}>({
  open: false,
  x: 0,
  y: 0,
  kind: "task",
  task: null,
  editEl: null,
  linkHref: "",
  selectionText: "",
});

const menuRoot = ref<HTMLElement | null>(null);
const menuPos = reactive({ x: 0, y: 0 });
let openedAt = 0;
const detailTaskId = ref<string | null>(null);
const limitTask = ref<DownloadTask | null>(null);
const removeTask = ref<DownloadTask | null>(null);
const saveAsTarget = ref<SaveAsTarget | null>(null);
const toastMessage = ref<string | null>(null);
let toastTimer: number | undefined;

function toast(message: string) {
  toastMessage.value = message;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => (toastMessage.value = null), 3200);
}

function closeMenu() {
  menu.open = false;
}

async function placeMenu(clientX: number, clientY: number) {
  menuPos.x = clientX;
  menuPos.y = clientY;
  await nextTick();
  const el = menuRoot.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const maxX = Math.max(8, window.innerWidth - rect.width - 8);
  const maxY = Math.max(8, window.innerHeight - rect.height - 8);
  menuPos.x = Math.min(clientX, maxX);
  menuPos.y = Math.min(clientY, maxY);
}

function openMenu(x: number, y: number, kind: MenuKind, extra?: Partial<typeof menu>) {
  menu.kind = kind;
  if (extra?.task !== undefined) menu.task = extra.task;
  if (extra?.editEl !== undefined) menu.editEl = extra.editEl;
  if (extra?.linkHref !== undefined) menu.linkHref = extra.linkHref;
  if (extra?.selectionText !== undefined) menu.selectionText = extra.selectionText;
  openedAt = performance.now();
  menu.open = true;
  void placeMenu(x, y);
}

function findTask(row: Element | null): DownloadTask | null {
  const id = (row as HTMLElement | null)?.dataset.id;
  if (!id) return null;
  return store.tasks.find((task) => task.id === id) ?? null;
}

function onContextMenu(event: MouseEvent) {
  const target = event.target as HTMLElement | null;
  if (!target) return;
  // Always suppress the browser menu inside the app (desktop + web).
  event.preventDefault();
  closeMenu();

  const row = target.closest?.(".task-row") ?? null;
  const task = findTask(row);
  if (task) {
    openMenu(event.clientX, event.clientY, "task", { task, editEl: null, linkHref: "", selectionText: "" });
    return;
  }

  const editEl = target.closest?.('input, textarea, [contenteditable="true"]') as
    | HTMLInputElement
    | HTMLTextAreaElement
    | HTMLElement
    | null;
  if (editEl && !(editEl as HTMLInputElement).disabled && !(editEl as HTMLInputElement).readOnly) {
    openMenu(event.clientX, event.clientY, "edit", { editEl, task: null, linkHref: "", selectionText: "" });
    return;
  }

  const selection = window.getSelection()?.toString().trim() ?? "";
  if (selection) {
    openMenu(event.clientX, event.clientY, "selection", { selectionText: selection, task: null, editEl: null, linkHref: "" });
    return;
  }

  const link = target.closest?.("a[href]") as HTMLAnchorElement | null;
  if (link?.href) {
    openMenu(event.clientX, event.clientY, "link", { linkHref: link.href, task: null, editEl: null, selectionText: "" });
    return;
  }
  // Nothing useful to offer — menu stays closed (browser menu suppressed regardless).
}

function onPointerDownCapture(event: MouseEvent) {
  if (!menu.open) return;
  const target = event.target as Node | null;
  if (target && menuRoot.value?.contains(target)) return;
  closeMenu();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") closeMenu();
}

function onCloseAll() {
  closeMenu();
}

/**
 * Chromium emits a spurious `scroll` right after a trusted right-click on
 * editable elements (caret scrolling). Closing on it makes the menu flash
 * and disappear, so transient post-open scrolls are ignored; genuine user
 * scrolls happen well after and still close the menu.
 */
function onScrollClose() {
  if (performance.now() - openedAt < 150) return;
  closeMenu();
}

onMounted(() => {
  window.addEventListener("contextmenu", onContextMenu);
  window.addEventListener("mousedown", onPointerDownCapture, true);
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("resize", onCloseAll);
  window.addEventListener("blur", onCloseAll);
  window.addEventListener("scroll", onScrollClose, true);
});
onBeforeUnmount(() => {
  window.removeEventListener("contextmenu", onContextMenu);
  window.removeEventListener("mousedown", onPointerDownCapture, true);
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("resize", onCloseAll);
  window.removeEventListener("blur", onCloseAll);
  window.removeEventListener("scroll", onScrollClose, true);
});

// ---------- helpers ----------

async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.style.position = "fixed";
    el.style.opacity = "0";
    document.body.appendChild(el);
    el.select();
    let ok = false;
    try {
      ok = document.execCommand("copy");
    } finally {
      el.remove();
    }
    return ok;
  }
}

function isEditableInput(el: HTMLInputElement | HTMLTextAreaElement | HTMLElement | null): el is HTMLInputElement | HTMLTextAreaElement {
  return el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement;
}

async function pasteInto(el: HTMLInputElement | HTMLTextAreaElement | HTMLElement | null) {
  if (!el) return;
  (el as HTMLElement).focus();
  let text: string | null = null;
  try {
    text = await navigator.clipboard.readText();
  } catch {
    text = null;
  }
  if (text == null) {
    toast(t("context.pasteDenied"));
    return;
  }
  if (!isEditableInput(el)) {
    if (document.execCommand("insertText", false, text)) return;
    toast(t("context.pasteDenied"));
    return;
  }
  if (document.execCommand("insertText", false, text)) return;
  // Fallback: write through the prototype setter so Vue's v-model sees the change,
  // inserting at the caret.
  const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
  if (!setter) return;
  const start = el.selectionStart ?? el.value.length;
  const end = el.selectionEnd ?? start;
  const next = el.value.slice(0, start) + text + el.value.slice(end);
  setter.call(el, next);
  el.setSelectionRange(start + text.length, start + text.length);
  el.dispatchEvent(new Event("input", { bubbles: true }));
}

function editAction(action: "cut" | "copy" | "paste" | "selectAll") {
  const el = menu.editEl;
  closeMenu();
  if (!el) return;
  if (action === "paste") {
    void pasteInto(el);
    return;
  }
  (el as HTMLElement).focus();
  if (action === "selectAll") {
    if (isEditableInput(el)) el.select();
    else document.execCommand("selectAll");
    return;
  }
  document.execCommand(action);
}

const taskPrimaryAction = computed(() => {
  const status = menu.task?.status;
  if (!status) return null;
  if (status === "paused" || status === "waiting" || status === "retrying")
    return { labelKey: "context.resume", run: "resume" as const };
  if (status === "pending") return { labelKey: "context.start", run: "resume" as const };
  if (["queued", "connecting", "downloading"].includes(status))
    return { labelKey: "context.pause", run: "pause" as const };
  return null;
});

const taskCanRetry = computed(() => {
  const status = menu.task?.status;
  return status === "failed" || status === "cancelled";
});

async function runTaskAction(action: "pause" | "resume" | "retry") {
  const task = menu.task;
  closeMenu();
  if (!task) return;
  try {
    if (action === "pause") await api.pause(task.id);
    else if (action === "resume") await api.resume(task.id);
    else await api.retry(task.id);
    await store.refresh();
  } catch (err) {
    toast(err instanceof Error ? err.message : String(err));
  }
}

async function copyTaskLink() {
  const task = menu.task;
  closeMenu();
  if (!task) return;
  const ok = await copyText(task.finalUrl ?? task.url);
  toast(t(ok ? "context.copiedLink" : "context.copyFailed"));
}

async function copySelection() {
  const text = menu.selectionText;
  closeMenu();
  const ok = await copyText(text);
  toast(t(ok ? "context.copied" : "context.copyFailed"));
}

async function copyLink() {
  const href = menu.linkHref;
  closeMenu();
  const ok = await copyText(href);
  toast(t(ok ? "context.copiedLink" : "context.copyFailed"));
}

function openDetail() {
  const task = menu.task;
  closeMenu();
  if (task) detailTaskId.value = task.id;
}

function openSaveAs() {
  const task = menu.task;
  closeMenu();
  if (task) saveAsTarget.value = { kind: "task", id: task.id, filename: task.filename };
}

async function downloadCompleted() {
  const task = menu.task;
  if (!task) return;
  closeMenu();
  if (apiToken()) toast(t("saveAs.browserDownloading"));
  try { await downloadTaskFile(task); }
  catch (err) { toast(err instanceof Error ? err.message : String(err)); }
}

function openLimit() {
  const task = menu.task;
  closeMenu();
  if (task) limitTask.value = task;
}

function askRemove() {
  const task = menu.task;
  closeMenu();
  if (task) removeTask.value = task;
}

async function onLimitSaved(restarted: boolean) {
  limitTask.value = null;
  toast(t(restarted ? "speedLimit.restarted" : "speedLimit.saved"));
  await store.refresh();
}

async function onRemoveConfirm(deleteFile: boolean) {
  const task = removeTask.value;
  removeTask.value = null;
  if (!task) return;
  await store.remove(task, deleteFile);
}
</script>

<template>
  <Teleport to="body">
    <Transition name="menu">
    <div v-if="menu.open" ref="menuRoot" class="context-menu af-panel" role="menu" :style="{ left: menuPos.x + 'px', top: menuPos.y + 'px' }">
      <!-- task actions -->
      <template v-if="menu.kind === 'task' && menu.task">
        <button class="menu-item" role="menuitem" @click="openDetail">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><circle cx="12" cy="12" r="9" stroke="currentColor" stroke-width="1.6"/><path d="M12 11v6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/><circle cx="12" cy="7.6" r="1.05" fill="currentColor"/></svg>
          {{ t("context.detail") }}
        </button>
        <button v-if="taskPrimaryAction" class="menu-item" role="menuitem" @click="runTaskAction(taskPrimaryAction.run)">
          <svg v-if="taskPrimaryAction.run === 'pause'" viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M9 6v12M15 6v12" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/></svg>
          <svg v-else viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M8 5.5v13l11-6.5-11-6.5Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/></svg>
          {{ t(taskPrimaryAction.labelKey) }}
        </button>
        <button v-if="taskCanRetry" class="menu-item" role="menuitem" @click="runTaskAction('retry')">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4 9.4M4 4v5.4h5.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/></svg>
          {{ t("context.retry") }}
        </button>
        <template v-if="menu.task.status === 'completed'">
          <div class="menu-sep" role="separator" />
          <button v-if="!desktop" class="menu-item" role="menuitem" @click="downloadCompleted">⬇ {{ t("task.saveFile") }}</button>
          <button class="menu-item" role="menuitem" @click="openSaveAs">💾 {{ t("saveAs.title") }}…</button>
        </template>
        <div class="menu-sep" role="separator" />
        <button class="menu-item" role="menuitem" @click="openLimit">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><circle cx="12" cy="12" r="8.5" stroke="currentColor" stroke-width="1.6"/><path d="M12 7.5V12l3 2.2" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
          {{ t("context.limitRate") }}
        </button>
        <button class="menu-item" role="menuitem" @click="copyTaskLink">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M10.6 13.4a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1.7 1.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M13.4 10.6a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1.7-1.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
          {{ t("context.copyLink") }}
        </button>
        <div class="menu-sep" role="separator" />
        <button class="menu-item danger" role="menuitem" @click="askRemove">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M4.5 6.5h15M9.5 6V4.8A1.3 1.3 0 0 1 10.8 3.5h2.4a1.3 1.3 0 0 1 1.3 1.3V6m-9.6.6 1 12a1.5 1.5 0 0 0 1.5 1.4h7.2a1.5 1.5 0 0 0 1.5-1.4l1-12" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
          {{ t("context.remove") }}
        </button>
      </template>

      <!-- editable field actions -->
      <template v-else-if="menu.kind === 'edit'">
        <button class="menu-item" role="menuitem" @click="editAction('cut')">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><circle cx="6.5" cy="6.5" r="2.4" stroke="currentColor" stroke-width="1.5"/><circle cx="6.5" cy="17.5" r="2.4" stroke="currentColor" stroke-width="1.5"/><path d="M8.4 8.2 20 19M8.4 15.8 20 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          {{ t("context.cut") }}
        </button>
        <button class="menu-item" role="menuitem" @click="editAction('copy')">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="1.6" stroke="currentColor" stroke-width="1.5"/><path d="M5.5 15V5.5A1.5 1.5 0 0 1 7 4h9.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          {{ t("context.copy") }}
        </button>
        <button class="menu-item" role="menuitem" @click="editAction('paste')">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><rect x="6" y="4.8" width="12" height="16" rx="1.6" stroke="currentColor" stroke-width="1.5"/><path d="M9.5 4.8a2.3 2.3 0 0 1 5 0" stroke="currentColor" stroke-width="1.5"/></svg>
          {{ t("context.paste") }}
        </button>
        <div class="menu-sep" role="separator" />
        <button class="menu-item" role="menuitem" @click="editAction('selectAll')">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M4 9V5.5A1.5 1.5 0 0 1 5.5 4H9M15 4h3.5A1.5 1.5 0 0 1 20 5.5V9M20 15v3.5a1.5 1.5 0 0 1-1.5 1.5H15M9 20H5.5A1.5 1.5 0 0 1 4 18.5V15" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          {{ t("context.selectAll") }}
        </button>
      </template>

      <!-- plain text selection -->
      <template v-else-if="menu.kind === 'selection'">
        <button class="menu-item" role="menuitem" @click="copySelection">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><rect x="9" y="9" width="11" height="11" rx="1.6" stroke="currentColor" stroke-width="1.5"/><path d="M5.5 15V5.5A1.5 1.5 0 0 1 7 4h9.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
          {{ t("context.copy") }}
        </button>
      </template>

      <!-- link -->
      <template v-else-if="menu.kind === 'link'">
        <button class="menu-item" role="menuitem" @click="copyLink">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M10.6 13.4a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1.7 1.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><path d="M13.4 10.6a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1.7-1.7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
          {{ t("context.copyLink") }}
        </button>
      </template>
    </div>
    </Transition>

    <!-- toast -->
    <Transition name="toast">
      <div v-if="toastMessage" class="context-toast" role="status">{{ toastMessage }}</div>
    </Transition>
  </Teleport>

  <SaveAsDialog :target="saveAsTarget" @close="saveAsTarget = null" />
  <TaskDetailDialog :task-id="detailTaskId" @close="detailTaskId = null" />
  <SpeedLimitDialog :task="limitTask" @close="limitTask = null" @saved="onLimitSaved" />
  <ConfirmDialog
    :open="removeTask !== null"
    :title="t('task.remove')"
    :message="t('task.removeConfirm', { name: removeTask?.filename ?? '' })"
    :danger="true"
    :checkbox-label="t('bulk.deleteFileToo')"
    :checkbox-default="store.deleteFilesOnRemove"
    @close="removeTask = null"
    @confirm="onRemoveConfirm"
  />
</template>

<style scoped>
.context-menu {
  position: fixed;
  z-index: 200;
  min-width: 216px;
  padding: 5px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  background: var(--af-panel-solid);
  border: 1px solid var(--af-border);
  border-radius: var(--af-radius-sm);
  box-shadow: var(--af-shadow);
  animation: menu-in 0.12s ease;
}
@keyframes menu-in {
  from {
    opacity: 0;
    transform: translateY(-3px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.menu-item {
  display: flex;
  align-items: center;
  gap: 9px;
  border: 0;
  background: transparent;
  color: var(--af-text);
  font-size: 12.5px;
  padding: 7px 10px;
  border-radius: var(--af-radius-sm);
  cursor: pointer;
  text-align: left;
}
.menu-item:hover {
  background: var(--af-accent-soft);
  color: var(--af-accent);
}
.menu-item.danger {
  color: var(--af-danger);
}
.menu-item.danger:hover {
  background: rgba(255, 59, 48, 0.12);
  color: var(--af-danger);
}
.menu-sep {
  height: 1px;
  background: var(--af-border);
  margin: 3px 6px;
}
.context-toast {
  position: fixed;
  z-index: 210;
  left: 50%;
  bottom: 34px;
  transform: translateX(-50%);
  background: var(--af-panel-solid);
  border: 1px solid var(--af-border);
  border-radius: 999px;
  box-shadow: var(--af-shadow-sm);
  padding: 7px 16px;
  font-size: 12.5px;
  color: var(--af-text);
  animation: menu-in 0.15s ease;
}
</style>
