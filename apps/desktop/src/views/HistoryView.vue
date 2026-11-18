<script setup lang="ts">
import { onMounted, ref, computed, reactive, watch } from "vue";
import { api, humanBytes, humanSpeed, downloadHistoryFile, isTauriEnv, apiToken, type HistoryEntry, type SaveAsTarget } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { showNotice } from "../lib/notice";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import SaveAsDialog from "../components/SaveAsDialog.vue";

const { t } = useI18n();
const desktop = isTauriEnv();

const entries = ref<HistoryEntry[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const limit = ref(100);
const readding = ref<string | null>(null);
const savingBrowserId = ref<number | null>(null);
const saveAsTarget = ref<SaveAsTarget | null>(null);
const deleting = ref(false);

// ------- selection (multi-select delete / clear all) -------
const selected = reactive(new Set<number>());
const selectedCount = computed(() => selected.size);
const allSelected = computed(() => entries.value.length > 0 && entries.value.every((e) => selected.has(e.id)));
const someSelected = computed(() => entries.value.some((e) => selected.has(e.id)));

watch(
  entries,
  () => {
    const live = new Set(entries.value.map((e) => e.id));
    for (const id of [...selected]) {
      if (!live.has(id)) selected.delete(id);
    }
  },
  { deep: false },
);

function toggleAll() {
  if (allSelected.value) {
    for (const entry of entries.value) selected.delete(entry.id);
  } else {
    for (const entry of entries.value) selected.add(entry.id);
  }
}

function toggleOne(id: number) {
  if (selected.has(id)) selected.delete(id);
  else selected.add(id);
}

async function load(nextLimit = limit.value) {
  loading.value = true;
  error.value = null;
  limit.value = nextLimit;
  try {
    entries.value = await api.history(nextLimit);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

async function saveBrowser(entry: HistoryEntry) {
  if (savingBrowserId.value !== null) return;
  savingBrowserId.value = entry.id;
  if (apiToken()) showNotice(t("saveAs.browserDownloading"));
  try {
    await downloadHistoryFile(entry);
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
  } finally {
    savingBrowserId.value = null;
  }
}

async function readd(entry: HistoryEntry) {
  readding.value = entry.url;
  try {
    await api.add(entry.url);
  } finally {
    readding.value = null;
  }
}

// ------- deletion (with confirmation) -------
const confirmState = reactive<{ open: boolean; mode: "selected" | "all" }>({ open: false, mode: "selected" });
const confirmTitle = computed(() =>
  confirmState.mode === "all" ? t("history.clearAllTitle") : t("history.deleteSelectedTitle"),
);
const confirmMessage = computed(() =>
  confirmState.mode === "all"
    ? t("history.clearAllMessage", { count: entries.value.length })
    : t("history.deleteSelectedMessage", { count: selectedCount.value }),
);

function askDeleteSelected() {
  if (selectedCount.value === 0) return;
  Object.assign(confirmState, { open: true, mode: "selected" });
}

function askClearAll() {
  Object.assign(confirmState, { open: true, mode: "all" });
}

async function onConfirmDelete() {
  const mode = confirmState.mode;
  confirmState.open = false;
  deleting.value = true;
  error.value = null;
  try {
    if (mode === "all") await api.historyDelete();
    else await api.historyDelete([...selected]);
    selected.clear();
    await load(limit.value);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    deleting.value = false;
  }
}

function resultClass(result: string): string {
  if (result === "completed") return "ok";
  if (result === "cancelled") return "cancelled";
  return "failed";
}

onMounted(() => load());
</script>

<template>
  <div class="history-view">
    <p v-if="error" class="error">{{ error }}</p>
    <p v-else-if="loading && entries.length === 0" class="dim">{{ t("settings.download.loading") }}</p>

    <div v-else-if="entries.length === 0" class="empty af-panel">
      <div class="empty-art">↺</div>
      <p>{{ t("history.emptyTitle") }}</p>
      <p class="dim">{{ t("history.emptyHint") }}</p>
    </div>

    <template v-else>
      <div class="bulk-bar af-panel">
        <label class="bulk-check">
          <input
            type="checkbox"
            :checked="allSelected"
            :indeterminate.prop="!allSelected && someSelected"
            @change="toggleAll"
          />
          <span>{{ t("bulk.selectAll") }}<template v-if="selectedCount > 0"> · {{ t("bulk.selected", { count: selectedCount }) }}</template></span>
        </label>
        <span class="bulk-spacer" />
        <button class="af-btn bulk-btn" :disabled="selectedCount === 0 || deleting" @click="askDeleteSelected">
          {{ t("history.deleteSelected", { count: selectedCount }) }}
        </button>
        <button class="af-btn bulk-btn danger-text" :disabled="deleting" @click="askClearAll">
          {{ t("history.clearAll") }}
        </button>
      </div>
      <div class="table-scroll">
      <table class="history-table af-panel">
        <thead>
          <tr>
            <th class="select-cell"><input type="checkbox" :checked="allSelected" :indeterminate.prop="!allSelected && someSelected" :aria-label="t('bulk.selectAll')" @change="toggleAll" /></th>
            <th>{{ t("history.col.filename") }}</th>
            <th>{{ t("history.col.result") }}</th>
            <th>{{ t("history.col.size") }}</th>
            <th>{{ t("history.col.avgSpeed") }}</th>
            <th>{{ t("history.col.completedAt") }}</th>
            <th>{{ t("history.col.actions") }}</th>
          </tr>
        </thead>
        <tbody>
          <tr v-for="entry in entries" :key="entry.id" :class="{ rowSelected: selected.has(entry.id) }">
            <td class="select-cell">
              <input type="checkbox" :checked="selected.has(entry.id)" :aria-label="t('bulk.selectRow', { name: entry.filename })" @change="toggleOne(entry.id)" />
            </td>
            <td class="filename" :title="entry.url">{{ entry.filename }}</td>
            <td>
              <span class="result-badge" :data-result="resultClass(entry.result)" :title="entry.errorMessage ?? undefined">
                {{ t(`status.${entry.result}`) }}
              </span>
            </td>
            <td>{{ humanBytes(entry.totalSize) }}</td>
            <td>{{ entry.averageSpeed != null ? humanSpeed(entry.averageSpeed) : "--" }}</td>
            <td class="mono dim">{{ entry.completedAt.replace("T", " ").replace(/\+.*$/, "").slice(0, 19) }}</td>
            <td>
              <div class="action-cell">
                <button v-if="entry.result === 'completed' && !desktop" class="mini-btn" :disabled="savingBrowserId === entry.id" :title="t('history.saveFile')" @click="saveBrowser(entry)">
                  ⬇
                </button>
                <button v-if="entry.result === 'completed'" class="mini-btn" :title="t('history.saveAs')" @click="saveAsTarget = { kind: 'history', id: entry.id, filename: entry.filename }">
                  💾
                </button>
                <button class="mini-btn" :disabled="readding === entry.url" @click="readd(entry)">
                  {{ t("history.readd") }}
                </button>
              </div>
            </td>
          </tr>
        </tbody>
      </table>
      </div>
      <div v-if="entries.length >= limit" class="row">
        <button class="af-btn" @click="load(limit + 100)">{{ t("history.loadMore") }}</button>
      </div>
    </template>

    <SaveAsDialog :target="saveAsTarget" @close="saveAsTarget = null" />
    <ConfirmDialog
      :open="confirmState.open"
      :title="confirmTitle"
      :message="confirmMessage"
      :danger="true"
      @close="confirmState.open = false"
      @confirm="onConfirmDelete"
    />
  </div>
</template>

<style scoped>
.history-view {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.list-head {
  font-size: 12px;
}
.dim {
  color: var(--af-text-dim);
  font-size: 12px;
}
.error {
  color: var(--af-danger);
  font-size: 13px;
}
.history-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  padding: 4px 0;
}
.history-table th,
.history-table td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--af-border);
}
.history-table tr:last-child td {
  border-bottom: 0;
}
@media (max-width: 767px) {
  .history-table {
    min-width: 720px;
  }
}
.history-table th {
  color: var(--af-text-dim);
  font-size: 11px;
  font-weight: 600;
}
.filename {
  max-width: 320px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-weight: 600;
}
.mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
.result-badge {
  font-size: 11px;
  padding: 2px 8px;
  border-radius: 999px;
}
.result-badge[data-result="ok"] {
  background: rgba(52, 199, 89, 0.16);
  color: var(--af-success);
}
.result-badge[data-result="failed"] {
  background: rgba(255, 59, 48, 0.14);
  color: var(--af-danger);
}
.action-cell {
  display: flex;
  align-items: center;
  gap: 6px;
}
.result-badge[data-result="cancelled"] {
  background: rgba(142, 142, 147, 0.2);
  color: var(--af-text-dim);
}
.empty {
  text-align: center;
  padding: 48px 16px;
  color: var(--af-text-dim);
}
.empty-art {
  font-size: 36px;
  opacity: 0.35;
}
.row {
  display: flex;
  justify-content: center;
}
.bulk-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  font-size: 12px;
  flex-wrap: wrap;
}
.bulk-check {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  color: var(--af-text-dim);
  cursor: pointer;
  user-select: none;
}
.bulk-spacer {
  flex: 1;
}
.bulk-btn {
  padding: 5px 11px;
  font-size: 12px;
}
.bulk-btn:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
.danger-text {
  color: var(--af-danger);
}
.select-cell {
  width: 36px;
  padding-left: 12px !important;
}
.rowSelected td {
  background: color-mix(in srgb, var(--af-accent) 7%, transparent);
}
.mini-btn {
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text-dim);
  border-radius: 5px;
  font-size: 11px;
  padding: 4px 10px;
  cursor: pointer;
}
.mini-btn:hover:not(:disabled) {
  color: var(--af-accent);
  border-color: var(--af-accent);
}
</style>
