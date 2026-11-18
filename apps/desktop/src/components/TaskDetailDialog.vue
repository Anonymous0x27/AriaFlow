<script setup lang="ts">
/**
 * Task detail dialog — everything the daemon knows about one task, including
 * per-segment transfer state. Values are selectable text (the app otherwise
 * disables selection) so the URL / save path / error can be copied.
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { api, humanBytes, humanEta, humanSpeed, etaSeconds, isTauriEnv, type DownloadTask } from "../lib/api";
import { statusLabel } from "../lib/i18n";
import { useI18n } from "../lib/i18n";

const props = defineProps<{
  /** Task id to display; `null` = closed. */
  taskId: string | null;
}>();
const emit = defineEmits<{ close: [] }>();

const { t } = useI18n();

const task = ref<DownloadTask | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
const actionError = ref<string | null>(null);
const actionBusy = ref(false);
const copiedField = ref<string | null>(null);
const isTauri = isTauriEnv();
let pollTimer: number | undefined;

async function load() {
  if (!props.taskId) return;
  try {
    task.value = await api.get(props.taskId);
    error.value = null;
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  }
}

watch(
  () => props.taskId,
  (id) => {
    window.clearInterval(pollTimer);
    task.value = null;
    error.value = null;
    if (id) {
      loading.value = true;
      void load().finally(() => (loading.value = false));
      pollTimer = window.setInterval(() => void load(), 1000);
    }
  },
);

onBeforeUnmount(() => window.clearInterval(pollTimer));

const open = computed(() => props.taskId !== null);

const canStart = computed(() =>
  ["pending", "paused", "waiting", "retrying"].includes(task.value?.status ?? ""),
);
const canPause = computed(() =>
  ["queued", "connecting", "downloading"].includes(task.value?.status ?? ""),
);

async function runAction(kind: "start" | "pause") {
  const id = props.taskId;
  if (!id || actionBusy.value) return;
  actionBusy.value = true;
  actionError.value = null;
  try {
    if (kind === "start") await api.resume(id);
    else await api.pause(id);
    await load();
  } catch (err) {
    actionError.value = err instanceof Error ? err.message : String(err);
  } finally {
    actionBusy.value = false;
  }
}

function close() {
  emit("close");
}

async function copy(text: string, field: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const el = document.createElement("textarea");
    el.value = text;
    el.style.position = "fixed";
    el.style.opacity = "0";
    document.body.appendChild(el);
    el.select();
    try {
      document.execCommand("copy");
    } finally {
      el.remove();
    }
  }
  copiedField.value = field;
  window.setTimeout(() => (copiedField.value = null), 1500);
}

const progressOf = (t_: DownloadTask) =>
  t_.status === "completed" ? 100 : Math.min(100, Math.max(0, t_.progress ?? 0));

const segmentRows = computed(() =>
  (task.value?.segments ?? []).map((seg) => {
    const size = seg.endOffset - seg.startOffset + 1;
    const pct = size > 0 ? Math.min(100, (seg.downloadedSize / size) * 100) : 0;
    return { ...seg, size, pct };
  }),
);

function formatWhen(iso: string | null | undefined): string {
  if (!iso) return "--";
  return iso.replace("T", " ").replace(/\+.*$/, "").replace(/\.\d+Z?$/, "").slice(0, 19);
}

function checksumOf(t_: DownloadTask | null): string {
  const c = t_?.checksum;
  if (!c) return "--";
  if (typeof c === "string") return c;
  return c.algorithm && c.digest ? `${c.algorithm}: ${c.digest}` : JSON.stringify(c);
}

function limitLabel(bytesPerSec: number | null | undefined): string {
  if (bytesPerSec == null) return t("detail.limitInherited");
  if (bytesPerSec === 0) return t("detail.limitUnlimited");
  return `${humanBytes(bytesPerSec)}/s`;
}
</script>

<template>
  <Transition name="dialog">
    <div v-if="open" class="dialog-backdrop" @click.self="close">
    <div
      class="dialog detail-dialog af-panel"
      role="dialog"
      aria-modal="true"
      :aria-label="t('detail.title')"
      @keydown.escape="close"
    >
      <div class="detail-head">
        <div class="detail-icon" aria-hidden="true">{{ task?.filename.split(".").pop()?.toUpperCase().slice(0, 4) || "FILE" }}</div>
        <div class="detail-headline">
          <h2 class="detail-filename" :title="task?.filename">{{ task?.filename ?? "…" }}</h2>
          <span v-if="task" class="status-badge" :data-status="task.status">{{ statusLabel(task.status) }}</span>
        </div>
        <button class="af-btn icon-btn detail-close" :title="t('common.close')" :aria-label="t('common.close')" @click="close">✕</button>
      </div>

      <p v-if="error" class="error-text">{{ error }}</p>
      <div v-else-if="loading && !task" class="dim loading-hint">{{ t("settings.download.loading") }}</div>

      <template v-else-if="task">
        <div class="af-progress big" :title="`${progressOf(task).toFixed(1)}%`">
          <div :style="{ width: progressOf(task) + '%' }" />
        </div>
        <div class="detail-body" :class="{ 'af-selectable': !isTauri }">
          <div class="detail-grid">
            <span class="k">{{ t("detail.url") }}</span>
            <span class="v mono break">
              {{ task.url }}
              <button class="mini-copy" :title="t('context.copy')" @click="copy(task.url, 'url')">{{ copiedField === "url" ? "✓" : "⧉" }}</button>
            </span>
            <template v-if="task.finalUrl && task.finalUrl !== task.url">
              <span class="k">{{ t("detail.finalUrl") }}</span>
              <span class="v mono break">
                {{ task.finalUrl }}
                <button class="mini-copy" :title="t('context.copy')" @click="copy(task.finalUrl ?? '', 'finalUrl')">{{ copiedField === "finalUrl" ? "✓" : "⧉" }}</button>
              </span>
            </template>
            <span class="k">{{ t("detail.savePath") }}</span>
            <span class="v mono break">
              {{ task.savePath }}
              <button class="mini-copy" :title="t('context.copy')" @click="copy(task.savePath, 'savePath')">{{ copiedField === "savePath" ? "✓" : "⧉" }}</button>
            </span>
            <span class="k">{{ t("detail.size") }}</span>
            <span class="v">{{ humanBytes(task.downloadedSize) }} / {{ humanBytes(task.totalSize) }}（{{ progressOf(task).toFixed(1) }}%）</span>
            <span class="k">{{ t("detail.speed") }}</span>
            <span class="v">{{ humanSpeed(task.speedBytesPerSec) }}<span class="dim"> · {{ t("detail.avgSpeed") }} {{ humanSpeed(task.averageSpeedBytesPerSec) }}</span></span>
            <span class="k">{{ t("detail.eta") }}</span>
            <span class="v">{{ t("task.etaRemaining", { eta: humanEta(etaSeconds(task)) }) }}</span>
            <span class="k">{{ t("detail.priority") }}</span>
            <span class="v">{{ task.priority }}</span>
            <span class="k">{{ t("detail.connections") }}</span>
            <span class="v">{{ task.currentConnections }} / {{ task.maxConnections }}</span>
            <span class="k">{{ t("detail.limitRate") }}</span>
            <span class="v">{{ limitLabel(task.limitRateBytesPerSec) }}</span>
            <span class="k">{{ t("detail.category") }}</span>
            <span class="v">{{ task.category }}</span>
            <span class="k">{{ t("detail.retries") }}</span>
            <span class="v">{{ task.retryCount }}</span>
            <span class="k">{{ t("detail.etag") }}</span>
            <span class="v mono break">{{ task.etag ?? "--" }}</span>
            <span class="k">{{ t("detail.lastModified") }}</span>
            <span class="v">{{ task.lastModified ?? "--" }}</span>
            <span class="k">{{ t("detail.checksum") }}</span>
            <span class="v mono break">{{ checksumOf(task) }}</span>
            <span class="k">{{ t("detail.createdAt") }}</span>
            <span class="v">{{ formatWhen(task.createdAt) }}</span>
            <span class="k">{{ t("detail.startedAt") }}</span>
            <span class="v">{{ formatWhen(task.startedAt) }}</span>
            <span class="k">{{ t("detail.completedAt") }}</span>
            <span class="v">{{ formatWhen(task.completedAt) }}</span>
            <template v-if="task.errorMessage">
              <span class="k">{{ t("detail.error") }}</span>
              <span class="v error-text break">{{ task.errorMessage }}</span>
            </template>
          </div>

          <h3 class="segments-title">{{ t("detail.segments") }}</h3>
          <div v-if="segmentRows.length > 0" class="table-scroll">
          <table class="segments-table">
            <thead>
              <tr>
                <th>#</th>
                <th>{{ t("detail.segRange") }}</th>
                <th>{{ t("detail.segDownloaded") }}</th>
                <th>{{ t("detail.segProgress") }}</th>
                <th>{{ t("detail.segStatus") }}</th>
                <th>{{ t("detail.segRetries") }}</th>
              </tr>
            </thead>
            <tbody>
              <tr v-for="seg in segmentRows" :key="seg.id">
                <td class="mono dim">{{ seg.id }}</td>
                <td class="mono">{{ humanBytes(seg.startOffset) }}–{{ humanBytes(seg.endOffset) }}</td>
                <td class="mono">{{ humanBytes(seg.downloadedSize) }} / {{ humanBytes(seg.size) }}</td>
                <td class="seg-progress-cell">
                  <div class="af-progress tiny"><div :style="{ width: seg.pct + '%' }" /></div>
                </td>
                <td><span class="seg-status" :data-status="seg.status">{{ statusLabel(seg.status.toLowerCase()) }}</span></td>
                <td class="mono dim center">{{ seg.retryCount }}</td>
              </tr>
            </tbody>
          </table>
          </div>
          <p v-else class="dim no-segments">{{ t("detail.noSegments") }}</p>
        </div>
      </template>

      <footer class="detail-footer">
        <p v-if="actionError" class="error-text action-error">{{ actionError }}</p>
        <span class="spacer" />
        <button class="af-btn" :disabled="!canPause || actionBusy" :title="t('detail.pause')" @click="runAction('pause')">
          {{ t("detail.pause") }}
        </button>
        <button class="af-btn af-btn-primary" :disabled="!canStart || actionBusy" :title="t('detail.start')" @click="runAction('start')">
          {{ t("detail.start") }}
        </button>
      </footer>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 120;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow-y: auto;
  padding: 40px 16px 32px;
  background: rgba(0, 0, 0, 0.4);
}
.dialog {
  display: flex;
  flex-direction: column;
}
.status-badge {
  flex: none;
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
  background: var(--af-accent-soft);
  color: var(--af-accent);
}
.status-badge[data-status="completed"] { background: rgba(52, 199, 89, 0.16); color: var(--af-success); }
.status-badge[data-status="failed"], .status-badge[data-status="cancelled"] { background: rgba(255, 59, 48, 0.14); color: var(--af-danger); }
.status-badge[data-status="paused"] { background: rgba(255, 159, 10, 0.16); color: var(--af-warning); }
.detail-dialog {
  width: min(640px, 100%);
  max-height: min(82vh, 780px);
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 18px 20px;
}
.detail-head {
  display: flex;
  align-items: center;
  gap: 12px;
  min-width: 0;
}
.detail-icon {
  width: 46px;
  height: 46px;
  flex: none;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 800;
  color: #fff;
  background: linear-gradient(150deg, #6e7bff, #4338ca);
}
.detail-headline {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
  flex: 1;
}
.detail-filename {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.detail-close {
  margin-left: auto;
}
.detail-footer {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-top: 4px;
  padding-top: 12px;
  border-top: 1px solid var(--af-border);
}
.detail-footer .spacer {
  flex: 1;
}
.detail-footer .action-error {
  margin: 0;
}
.af-progress.big {
  height: 6px;
}
.af-progress.tiny {
  height: 4px;
  width: 90px;
}
.detail-body {
  overflow-y: auto;
  min-height: 0;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.detail-grid {
  display: grid;
  grid-template-columns: auto 1fr;
  gap: 5px 14px;
  font-size: 12px;
  align-items: baseline;
}
.detail-grid .k {
  color: var(--af-text-dim);
  white-space: nowrap;
}
.detail-grid .v {
  min-width: 0;
  word-break: break-word;
}
.mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
.break {
  word-break: break-all;
}
.dim {
  color: var(--af-text-dim);
}
.error-text {
  color: var(--af-danger);
}
.loading-hint {
  font-size: 12px;
  padding: 8px 2px;
}
.mini-copy {
  border: 0;
  background: transparent;
  color: var(--af-text-dim);
  cursor: pointer;
  font-size: 13px;
  padding: 0 4px;
  vertical-align: baseline;
}
.mini-copy:hover {
  color: var(--af-accent);
}
.segments-title {
  margin: 2px 0 0;
  font-size: 11px;
  font-weight: 650;
  color: var(--af-text-dim);
  letter-spacing: 0.4px;
  text-transform: uppercase;
}
.segments-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 11px;
}
@media (max-width: 767px) {
  .segments-table {
    min-width: 560px;
  }
}
.segments-table th,
.segments-table td {
  text-align: left;
  padding: 5px 8px;
  border-bottom: 1px solid var(--af-border);
}
.segments-table th {
  color: var(--af-text-dim);
  font-size: 10px;
  font-weight: 600;
}
.segments-table tr:last-child td {
  border-bottom: 0;
}
.seg-progress-cell {
  width: 110px;
}
.seg-status {
  font-size: 10px;
  padding: 1px 7px;
  border-radius: 999px;
  background: rgba(127, 127, 127, 0.16);
  color: var(--af-text-dim);
}
.seg-status[data-status="downloading"] {
  background: var(--af-accent-soft);
  color: var(--af-accent);
}
.seg-status[data-status="completed"] {
  background: rgba(52, 199, 89, 0.16);
  color: var(--af-success);
}
.seg-status[data-status="failed"] {
  background: rgba(255, 59, 48, 0.14);
  color: var(--af-danger);
}
.center {
  text-align: center;
}
.no-segments {
  font-size: 12px;
  margin: 0;
}
</style>
