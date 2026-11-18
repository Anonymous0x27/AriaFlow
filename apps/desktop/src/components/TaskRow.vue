<script setup lang="ts">
import { ref } from "vue";
import type { DownloadTask } from "../lib/api";
import { humanBytes, humanSpeed, humanEta, etaSeconds, downloadTaskFile, isTauriEnv, apiToken, type SaveAsTarget } from "../lib/api";
import { showNotice } from "../lib/notice";
import { useI18n, statusLabel, categoryLabel } from "../lib/i18n";
import SaveAsDialog from "./SaveAsDialog.vue";

const { t } = useI18n();
const desktop = isTauriEnv();
const saveAsTask = ref<SaveAsTarget | null>(null);

withDefaults(
  defineProps<{ task: DownloadTask; selectable?: boolean; selected?: boolean }>(),
  { selectable: false, selected: false },
);
const emit = defineEmits<{
  (e: "toggle-pause-resume", task: DownloadTask): void;
  (e: "remove", task: DownloadTask): void;
  (e: "retry", task: DownloadTask): void;
  (e: "toggle-select", task: DownloadTask): void;
}>();

const TERMINAL_STATUSES = new Set(["completed", "failed", "cancelled"]);

async function saveBrowser(task: DownloadTask) {
  if (apiToken()) showNotice(t("saveAs.browserDownloading"));
  try {
    await downloadTaskFile(task);
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
  }
}

function progressOf(task: DownloadTask): number {
  if (task.status === "completed") return 100;
  return Math.min(100, Math.max(0, task.progress ?? 0));
}
</script>

<template>
  <div class="task-row" :class="{ selectable }" :data-status="task.status" :data-id="task.id">
    <input
      v-if="selectable"
      type="checkbox"
      class="row-select"
      :checked="selected"
      :aria-label="t('bulk.selectRow', { name: task.filename })"
      @change="emit('toggle-select', task)"
    />
    <div class="file-icon" aria-hidden="true">{{ task.filename.split(".").pop()?.toUpperCase().slice(0, 4) || "FILE" }}</div>

    <div class="task-main">
      <div class="task-top">
        <span class="filename" :title="task.filename">{{ task.filename }}</span>
        <span class="status-badge" :data-status="task.status">{{ statusLabel(task.status) }}</span>
      </div>

      <div class="af-progress" :title="`${progressOf(task).toFixed(1)}%`">
        <div :style="{ width: progressOf(task) + '%' }" />
      </div>

      <div class="task-meta">
        <span>{{ humanBytes(task.downloadedSize) }} / {{ humanBytes(task.totalSize) }}</span>
        <span v-if="task.status === 'downloading'">· {{ humanSpeed(task.speedBytesPerSec) }}</span>
        <span v-if="task.status === 'downloading'">· {{ t("task.etaRemaining", { eta: humanEta(etaSeconds(task)) }) }}</span>
        <span v-if="task.status === 'failed' && task.errorMessage" class="error-text" :title="task.errorMessage">· {{ task.errorMessage }}</span>
        <span class="category">{{ categoryLabel(task.category) }}</span>
      </div>
    </div>

    <div class="task-actions">
      <button
        v-if="!TERMINAL_STATUSES.has(task.status)"
        class="af-btn icon-btn"
        :title="task.status === 'paused' ? t('task.resume') : t('task.pause')"
        @click="emit('toggle-pause-resume', task)"
      >{{ task.status === "paused" ? "▶" : "❚❚" }}</button>
      <button v-if="task.status === 'failed' || task.status === 'cancelled'" class="af-btn icon-btn" :title="t('task.retry')" @click="emit('retry', task)">↻</button>
      <button v-if="task.status === 'completed' && !desktop" class="af-btn icon-btn" :title="t('task.saveFile')" @click="saveBrowser(task)">⬇</button>
      <button v-if="task.status === 'completed'" class="af-btn icon-btn" :title="t('task.saveAs')" @click="saveAsTask = { kind: 'task', id: task.id, filename: task.filename }">💾</button>
      <button class="af-btn icon-btn danger" :title="t('task.remove')" @click="emit('remove', task)">✕</button>
    </div>
    <SaveAsDialog :target="saveAsTask" @close="saveAsTask = null" />
  </div>
</template>

<style scoped>
.task-row.selectable {
  grid-template-columns: auto 46px 1fr auto;
}
.row-select {
  align-self: center;
}

.task-row {
  display: grid;
  grid-template-columns: 46px 1fr auto;
  gap: 12px;
  align-items: center;
  padding: 12px 14px;
  background: var(--af-panel);
  backdrop-filter: blur(var(--af-blur));
  border: 1px solid var(--af-border);
  border-radius: var(--af-radius-sm);
  box-shadow: var(--af-shadow-sm);
}

.file-icon {
  height: 46px;
  border-radius: 10px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 10px;
  font-weight: 800;
  letter-spacing: 0.5px;
  color: #fff;
  background: linear-gradient(150deg, #6e7bff, #4338ca);
}
.task-row[data-status="completed"] .file-icon { background: linear-gradient(150deg, #30d158, #1f9d47); }
.task-row[data-status="failed"] .file-icon,
.task-row[data-status="cancelled"] .file-icon { background: linear-gradient(150deg, #ff6961, #d70015); }
.task-row[data-status="paused"] .file-icon { background: linear-gradient(150deg, #ffd60a, #ff9f0a); }
.task-row[data-status="downloading"] .file-icon { background: linear-gradient(150deg, #3fa2ff, #0060df); }

.task-main {
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.task-top {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.filename {
  font-size: 13px;
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
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

.task-meta {
  display: flex;
  flex-wrap: wrap;
  column-gap: 6px;
  row-gap: 2px;
  align-items: center;
  font-size: 11px;
  color: var(--af-text-dim);
  font-variant-numeric: tabular-nums;
  white-space: nowrap;
  overflow: hidden;
}
.task-meta .category {
  margin-left: auto;
  border: 1px solid var(--af-border);
  padding: 1px 6px;
  border-radius: 4px;
  font-size: 10px;
}
.error-text {
  color: var(--af-danger);
  max-width: 240px;
  overflow: hidden;
  text-overflow: ellipsis;
}

.task-actions {
  display: flex;
  gap: 6px;
}
.icon-btn {
  width: 32px;
  height: 32px;
  padding: 0;
  justify-content: center;
  font-size: 13px;
}
@media (max-width: 640px) {
  .task-row {
    grid-template-columns: 38px minmax(0, 1fr) auto;
    gap: 8px;
    padding: 10px;
  }
  .task-row.selectable {
    grid-template-columns: auto 38px minmax(0, 1fr) auto;
  }
  .file-icon {
    height: 38px;
  }
  .filename {
    font-size: 12px;
  }
}

.icon-btn.danger:hover {
  color: var(--af-danger);
  border-color: rgba(255, 59, 48, 0.4);
}
</style>
