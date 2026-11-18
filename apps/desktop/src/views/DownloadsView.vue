<script setup lang="ts">
import BatchAddDialog from "../components/BatchAddDialog.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import { onMounted, watch, ref, computed, reactive } from "vue";
import { useRoute } from "vue-router";
import { useDownloadsStore, type TaskFilter } from "../stores/downloads";
import { useI18n } from "../lib/i18n";
import TaskRow from "../components/TaskRow.vue";
import { api, type DownloadTask } from "../lib/api";
import { showNotice } from "../lib/notice";

const { t } = useI18n();

const route = useRoute();
const store = useDownloadsStore();
const url = ref("");
const batchOpen = ref(false);

function openBatch() {
  batchOpen.value = true;
}

async function onBatchAdded() {
  await store.refresh();
}

const tabs = computed<{ key: TaskFilter; label: string }[]>(() => [
  { key: "all", label: t("downloads.tab.all") },
  { key: "active", label: t("downloads.tab.active") },
  { key: "queued", label: t("downloads.tab.queued") },
  { key: "paused", label: t("downloads.tab.paused") },
  { key: "completed", label: t("downloads.tab.completed") },
  { key: "failed", label: t("downloads.tab.failed") },
]);

function filterFromRoute(): TaskFilter {
  const raw = String(route.params.filter ?? (route.query.filter as string) ?? "all");
  return tabs.value.some((tab) => tab.key === raw) ? (raw as TaskFilter) : "all";
}

onMounted(() => store.setFilter(filterFromRoute()));
watch(() => route.params.filter, () => store.setFilter(filterFromRoute()));
watch(() => store.search, () => void store.refresh());

const selected = reactive(new Set<string>());
const selectedCount = computed(() => selected.size);
const visibleIds = computed(() => store.visibleTasks.map((task) => task.id));
const allVisibleSelected = computed(
  () => visibleIds.value.length > 0 && visibleIds.value.every((id) => selected.has(id)),
);
const someVisibleSelected = computed(() => visibleIds.value.some((id) => selected.has(id)));

// Keep the selection consistent with what is visible (filter/search changes).
watch(visibleIds, () => {
  const visible = new Set(visibleIds.value);
  for (const id of [...selected]) {
    if (!visible.has(id)) selected.delete(id);
  }
});

function toggleSelect(task: DownloadTask) {
  if (selected.has(task.id)) selected.delete(task.id);
  else selected.add(task.id);
}

function toggleSelectAll() {
  if (allVisibleSelected.value) {
    for (const id of visibleIds.value) selected.delete(id);
  } else {
    for (const id of visibleIds.value) selected.add(id);
  }
}

function clearSelection() {
  selected.clear();
}

async function bulkAction(action: "pause" | "resume" | "retry") {
  const ids = [...selected];
  if (ids.length === 0) return;
  const { failed } = await store.bulk(ids, action);
  if (failed > 0) showNotice(t("bulk.partialFailure", { count: failed }));
}

const confirmState = reactive<{
  open: boolean;
  mode: "one" | "many" | "clear-completed";
  task: DownloadTask | null;
  count: number;
}>({ open: false, mode: "one", task: null, count: 0 });

const confirmTitle = computed(() => {
  if (confirmState.mode === "clear-completed") return t("downloads.clearCompletedTitle");
  return confirmState.mode === "many" ? t("bulk.deleteTitle") : t("task.remove");
});
const confirmMessage = computed(() => {
  if (confirmState.mode === "clear-completed")
    return t("downloads.clearCompletedMessage", { count: confirmState.count });
  if (confirmState.mode === "many") return t("bulk.deleteMessage", { count: confirmState.count });
  return t("task.removeConfirm", { name: confirmState.task?.filename ?? "" });
});

function askRemoveOne(task: DownloadTask) {
  Object.assign(confirmState, { open: true, mode: "one", task, count: 1 });
}

function askBulkDelete() {
  Object.assign(confirmState, { open: true, mode: "many", task: null, count: selectedCount.value });
}

async function askClearCompleted() {
  try {
    const completed = await api.list({ status: "completed" });
    Object.assign(confirmState, {
      open: true,
      mode: "clear-completed",
      task: null,
      count: completed.length,
    });
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
  }
}

const clearingCompleted = ref(false);

async function onConfirm(deleteFile: boolean) {
  const state = { ...confirmState };
  confirmState.open = false;
  confirmState.task = null;
  let failed = 0;
  if (state.mode === "one" && state.task) {
    failed = await store.bulk([state.task.id], "remove", { deleteFile }).then((r) => r.failed);
  } else if (state.mode === "many") {
    const ids = [...selected];
    const result = await store.bulk(ids, "remove", { deleteFile });
    failed = result.failed;
    if (failed === 0) selected.clear();
  } else if (state.mode === "clear-completed") {
    clearingCompleted.value = true;
    try {
      const completed = await api.list({ status: "completed" });
      const result = await store.bulk(completed.map((task) => task.id), "remove", { deleteFile });
      failed = result.failed;
    } finally {
      clearingCompleted.value = false;
    }
  }
  if (failed > 0) showNotice(t("bulk.partialFailure", { count: failed }));
}

const activeFilter = computed(() => store.filter);

async function submitAdd() {
  const value = url.value.trim();
  if (!value) return;
  try {
    await store.add(value);
    url.value = "";
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
  }
}
</script>

<template>
  <div class="downloads-view">
    <div class="controls">
      <form class="search" @submit.prevent="submitAdd">
        <input v-model="url" class="af-input" :placeholder="t('downloads.addPlaceholder')" spellcheck="false" />
      </form>
      <input v-model="store.search" class="af-input filter-input" :placeholder="t('downloads.searchPlaceholder')" spellcheck="false" />
    </div>

    <div class="bulk-bar af-panel" v-if="visibleIds.length > 0">
      <label class="bulk-check">
        <input
          type="checkbox"
          :checked="allVisibleSelected"
          :indeterminate.prop="!allVisibleSelected && someVisibleSelected"
          @change="toggleSelectAll"
        />
        <span>{{ t("bulk.selectAll") }}<template v-if="selectedCount > 0"> · {{ t("bulk.selected", { count: selectedCount }) }}</template></span>
      </label>
      <span class="bulk-spacer" />
      <button class="af-btn bulk-btn" :disabled="selectedCount === 0" @click="bulkAction('resume')">{{ t("bulk.resume") }}</button>
      <button class="af-btn bulk-btn" :disabled="selectedCount === 0" @click="bulkAction('pause')">{{ t("bulk.pause") }}</button>
      <button class="af-btn bulk-btn" :disabled="selectedCount === 0" @click="bulkAction('retry')">{{ t("bulk.retry") }}</button>
      <button class="af-btn bulk-btn" :disabled="selectedCount === 0" @click="askBulkDelete">{{ t("bulk.delete") }}</button>
      <button v-if="activeFilter === 'completed'" class="af-btn bulk-btn danger-text" :disabled="clearingCompleted" @click="askClearCompleted">{{ t("downloads.clearCompleted") }}</button>
      <button v-if="selectedCount > 0" class="af-btn bulk-btn" @click="clearSelection">{{ t("bulk.clearSelection") }}</button>
    </div>

    <div class="tabs">
      <button
        v-for="tab in tabs"
        :key="tab.key"
        class="tab"
        :class="{ active: activeFilter === tab.key }"
        @click="store.setFilter(tab.key)"
      >{{ tab.label }}</button>
      <span class="spacer" />
      <button class="af-btn" @click="openBatch">{{ t("overview.batchAdd") }}</button>
      <button class="af-btn" @click="store.refresh()">{{ t("downloads.refresh") }}</button>
    </div>

    <div class="task-list">
      <TaskRow
        v-for="task in store.visibleTasks"
        :key="task.id"
        :task="task"
        selectable
        :selected="selected.has(task.id)"
        @toggle-select="toggleSelect"
        @toggle-pause-resume="store.togglePauseResume($event)"
        @remove="askRemoveOne"
        @retry="store.retry($event)"
      />
      <div v-if="store.visibleTasks.length === 0" class="empty">{{ t("downloads.empty") }}</div>
    </div>

    <BatchAddDialog :open="batchOpen" @close="batchOpen = false" @added="onBatchAdded" />
    <ConfirmDialog
      :open="confirmState.open"
      :title="confirmTitle"
      :message="confirmMessage"
      :danger="true"
      :checkbox-label="t('bulk.deleteFileToo')"
      :checkbox-default="store.deleteFilesOnRemove"
      @close="confirmState.open = false"
      @confirm="onConfirm"
    />
  </div>
</template>

<style scoped>
.downloads-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.controls {
  display: flex;
  gap: 10px;
}
.search {
  flex: 1.2;
  display: flex;
}
.search .af-input {
  flex: 1;
}
.filter-input {
  flex: 1;
}
.tabs {
  display: flex;
  align-items: center;
  gap: 6px;
}
.tab {
  border: 0;
  background: transparent;
  color: var(--af-text-dim);
  padding: 6px 12px;
  border-radius: 999px;
  font-size: 12px;
  cursor: pointer;
}
.tab:hover {
  background: rgba(127, 127, 127, 0.12);
}
.tab.active {
  background: var(--af-accent-soft);
  color: var(--af-accent);
  font-weight: 650;
}
.spacer {
  flex: 1;
}
.task-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.empty {
  text-align: center;
  color: var(--af-text-dim);
  padding: 36px 0;
  font-size: 13px;
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
</style>
