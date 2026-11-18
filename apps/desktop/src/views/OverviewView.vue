<script setup lang="ts">
import { onMounted, onUnmounted, ref, computed, reactive } from "vue";
import { useRouter } from "vue-router";
import { useDownloadsStore } from "../stores/downloads";
import { api, humanBytes, humanSpeed } from "../lib/api";
import { showNotice } from "../lib/notice";
import { useI18n } from "../lib/i18n";
import TaskRow from "../components/TaskRow.vue";
import AddTaskDialog from "../components/AddTaskDialog.vue";
import BatchAddDialog from "../components/BatchAddDialog.vue";
import ConfirmDialog from "../components/ConfirmDialog.vue";
import type { DownloadTask } from "../lib/api";

const { t } = useI18n();

const store = useDownloadsStore();
const router = useRouter();
const url = ref("");
const adding = ref(false);
const addDialogOpen = ref(false);
const batchDialogOpen = ref(false);
const removeConfirm = reactive<{ open: boolean; task: DownloadTask | null }>({ open: false, task: null });
const showPaused = ref(false);
let showPausedLoaded = false;
/** Default checked; persisted in daemon settings (overviewShowNewTasks). */
const showNewTasks = ref(true);
let showNewTasksLoaded = false;

/** Cards hidden on the overview; persisted in daemon settings (overviewHiddenCards). */
const hiddenCards = ref<Set<CardKey>>(new Set());
const contentDialogOpen = ref(false);
let cardsLoaded = false;

async function toggleCard(key: CardKey, visible: boolean) {
  const previous = new Set(hiddenCards.value);
  const next = new Set(previous);
  if (visible) next.delete(key);
  else next.add(key);
  hiddenCards.value = next;
  try {
    await api.updateSettings({ overviewHiddenCards: [...next] });
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
    hiddenCards.value = previous; // daemon rejected the patch: revert
  }
}

async function applyShowNewTasks(value: boolean, persist: boolean) {
  showNewTasks.value = value;
  store.includeNewInActive = value;
  if (persist) {
    try {
      await api.updateSettings({ overviewShowNewTasks: value });
    } catch (err) {
      showNotice(err instanceof Error ? err.message : String(err));
      // Revert the local toggle if the daemon rejected the setting.
      showNewTasks.value = !value;
      store.includeNewInActive = !value;
      return;
    }
  }
  void store.refresh();
}

async function applyShowPaused(value: boolean, persist: boolean) {
  showPaused.value = value;
  store.includePausedInActive = value;
  if (persist) {
    try {
      await api.updateSettings({ overviewShowPaused: value });
    } catch (err) {
      showNotice(err instanceof Error ? err.message : String(err));
      // Revert the local toggle if the daemon rejected the setting.
      showPaused.value = !value;
      store.includePausedInActive = !value;
      return;
    }
  }
  void store.refresh();
}

function askRemove(task: DownloadTask) {
  removeConfirm.open = true;
  removeConfirm.task = task;
}

async function onRemoveConfirm(deleteFile: boolean) {
  const task = removeConfirm.task;
  removeConfirm.open = false;
  removeConfirm.task = null;
  if (task) await store.remove(task, deleteFile);
}

onMounted(async () => {
  try {
    const settings = await api.settings();
    showPaused.value = settings.overviewShowPaused;
    store.includePausedInActive = settings.overviewShowPaused;
    showPausedLoaded = true;
    showNewTasks.value = settings.overviewShowNewTasks ?? true;
    store.includeNewInActive = settings.overviewShowNewTasks ?? true;
    showNewTasksLoaded = true;
    const known = new Set(ALL_CARD_KEYS);
    hiddenCards.value = new Set(
      (settings.overviewHiddenCards ?? []).filter((id): id is CardKey => known.has(id as CardKey)),
    );
    cardsLoaded = true;
  } catch {
    showPausedLoaded = false; // offline: keep previous value, leave settings untouched
    showNewTasksLoaded = false;
    store.includeNewInActive = showNewTasks.value; // offline default: visible (checked)
    cardsLoaded = false; // offline: cards settings disabled, everything shown
  }
  store.setFilter("active");
});

onUnmounted(() => {
  // The include-paused flag is overview-only: never let it leak into the
  // shared store filter used by the Downloads views.
  store.includePausedInActive = false;
  store.includeNewInActive = false;
});

async function submitAdd() {
  if (!url.value.trim()) return;
  adding.value = true;
    try {
    await store.add(url.value);
    url.value = "";
  } catch (err) {
    showNotice(err instanceof Error ? err.message : String(err));
  } finally {
    adding.value = false;
  }
}

function onDialogAdded() {
  url.value = "";
  void store.refresh();
}

type CountKey = "downloading" | "waiting" | "paused" | "completed" | "failed";
type CardKey = CountKey | "currentSpeed" | "totalDownloaded";
const ALL_CARD_KEYS: readonly CardKey[] = [
  "downloading",
  "waiting",
  "paused",
  "completed",
  "failed",
  "currentSpeed",
  "totalDownloaded",
];

/** Where each card navigates on click. */
const CARD_ROUTES: Record<CardKey, string> = {
  downloading: "/downloads/active",
  waiting: "/downloads/queued",
  paused: "/downloads/paused",
  completed: "/downloads/completed",
  failed: "/downloads/failed",
  currentSpeed: "/downloads/active",
  totalDownloaded: "/history",
};

interface CardDef {
  key: CardKey;
  label: string;
  value: string;
  color: string | null;
  mono: boolean;
  to: string;
}

const cards = computed<{ key: CountKey; label: string; color: string }[]>(() => [
  { key: "downloading", label: t("overview.card.downloading"), color: "var(--af-accent)" },
  { key: "waiting", label: t("overview.card.waiting"), color: "var(--af-text-dim)" },
  { key: "paused", label: t("overview.card.paused"), color: "var(--af-warning)" },
  { key: "completed", label: t("overview.card.completed"), color: "var(--af-success)" },
  { key: "failed", label: t("overview.card.failed"), color: "var(--af-danger)" },
]);

const allCards = computed<CardDef[]>(() => [
  ...cards.value.map((card) => ({
    key: card.key as CardKey,
    label: card.label,
    value: String(store.counts?.[card.key] ?? 0),
    color: card.color,
    mono: false,
    to: CARD_ROUTES[card.key],
  })),
  {
    key: "currentSpeed",
    label: t("overview.currentSpeed"),
    value: humanSpeed(store.counts?.globalSpeedBytesPerSec ?? 0),
    color: null,
    mono: true,
    to: CARD_ROUTES.currentSpeed,
  },
  {
    key: "totalDownloaded",
    label: t("overview.totalDownloaded"),
    value: humanBytes(store.counts?.totalDownloadedBytes ?? 0),
    color: null,
    mono: true,
    to: CARD_ROUTES.totalDownloaded,
  },
]);
const visibleCards = computed(() => allCards.value.filter((card) => !hiddenCards.value.has(card.key)));
const cardToggleOptions = computed(() => allCards.value);
</script>

<template>
  <div class="overview">
    <!-- 添加栏 -->
    <form class="add-bar af-panel" @submit.prevent="submitAdd">
      <span class="add-icon">↓</span>
      <input
        v-model="url"
        class="af-input add-input"
        type="text"
        inputmode="url"
        :placeholder="t('overview.addPlaceholder')"
        spellcheck="false"
        autocomplete="off"
      />
      <button type="button" class="af-btn" :title="t('overview.advanced')" @click="addDialogOpen = true">
        {{ t("overview.advanced") }}
      </button>
      <button class="af-btn af-btn-primary" type="submit" :disabled="adding || !url.trim()">
        {{ adding ? t("overview.adding") : t("overview.addButton") }}
      </button>
    </form>
    <div class="toolbar-under">
      <button type="button" class="af-btn" @click="batchDialogOpen = true">{{ t("overview.batchAdd") }}</button>
    </div>

    <!-- 统计卡片（可在“显示内容”中关闭；点击跳转对应页面） -->
    <div v-if="visibleCards.length" class="stat-grid">
      <button
        v-for="card in visibleCards"
        :key="card.key"
        type="button"
        class="stat-card af-panel"
        :title="card.label"
        @click="router.push(card.to)"
      >
        <div class="stat-value" :class="{ mono: card.mono }" :style="card.color ? { color: card.color } : undefined">{{ card.value }}</div>
        <div class="stat-label">{{ card.label }}</div>
      </button>
    </div>

    <AddTaskDialog :open="addDialogOpen" :initial-url="url" @close="addDialogOpen = false" @added="onDialogAdded" />
    <BatchAddDialog :open="batchDialogOpen" @close="batchDialogOpen = false" @added="onDialogAdded" />
    <ConfirmDialog
      :open="removeConfirm.open"
      :title="t('task.remove')"
      :message="t('task.removeConfirm', { name: removeConfirm.task?.filename ?? '' })"
      :danger="true"
      :checkbox-label="t('bulk.deleteFileToo')"
      :checkbox-default="store.deleteFilesOnRemove"
      @close="removeConfirm.open = false"
      @confirm="onRemoveConfirm"
    />

    <!-- 活跃任务 -->
    <div class="section-head">
      <h2>{{ t("overview.inProgress") }}</h2>
      <div class="section-head-actions">
        <button type="button" class="af-btn content-btn" :title="t('overview.showContentHint')" @click="contentDialogOpen = true">
          <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><path d="M4 8h16M4 16h16" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="9" cy="8" r="2.1" fill="var(--af-panel-solid, #fff)" stroke="currentColor" stroke-width="1.6"/><circle cx="15" cy="16" r="2.1" fill="var(--af-panel-solid, #fff)" stroke="currentColor" stroke-width="1.6"/></svg>
          {{ t("overview.showContent") }}
        </button>
        <button class="af-btn" @click="router.push('/downloads/active')">{{ t("overview.viewAll") }}</button>
      </div>
    </div>
    <div class="task-list">
      <TaskRow
        v-for="task in store.visibleTasks"
        :key="task.id"
        :task="task"
        @toggle-pause-resume="store.togglePauseResume($event)"
        @remove="askRemove"
        @retry="store.retry($event)"
      />
      <div v-if="store.visibleTasks.length === 0" class="empty">
        <div class="empty-art">↓</div>
        <p>{{ t("overview.emptyTitle") }}</p>
        <p class="dim">{{ t("overview.emptyHint") }}</p>
      </div>
    </div>

    <!-- 显示内容设置 -->
    <Transition name="dialog">
      <div v-if="contentDialogOpen" class="dialog-backdrop" @click.self="contentDialogOpen = false">
        <div class="content-dialog af-panel" role="dialog" aria-modal="true" :aria-label="t('overview.showContentTitle')">
          <h2 class="content-title">{{ t("overview.showContentTitle") }}</h2>
          <p class="content-hint">{{ t("overview.showContentHint") }}</p>

          <div class="content-group">
            <div class="content-group-label">{{ t("overview.contentGroupCards") }}</div>
            <label v-for="card in cardToggleOptions" :key="card.key" class="check-row" :class="{ disabled: !cardsLoaded }">
              <input
                type="checkbox"
                :checked="!hiddenCards.has(card.key)"
                :disabled="!cardsLoaded"
                @change="toggleCard(card.key, ($event.target as HTMLInputElement).checked)"
              />
              <span>{{ card.label }}</span>
            </label>
          </div>

          <div class="content-group">
            <div class="content-group-label">{{ t("overview.contentGroupList") }}</div>
            <label class="check-row" :title="t('overview.showNewTasksHint')" :class="{ disabled: !showNewTasksLoaded }">
              <input
                type="checkbox"
                :checked="showNewTasks"
                :disabled="!showNewTasksLoaded"
                @change="applyShowNewTasks(($event.target as HTMLInputElement).checked, true)"
              />
              <span>{{ t("overview.showNewTasks") }}</span>
            </label>
            <label class="check-row" :title="t('overview.showPausedHint')" :class="{ disabled: !showPausedLoaded }">
              <input
                type="checkbox"
                :checked="showPaused"
                :disabled="!showPausedLoaded"
                @change="applyShowPaused(($event.target as HTMLInputElement).checked, true)"
              />
              <span>{{ t("overview.showPaused") }}</span>
            </label>
          </div>

          <div class="content-actions">
            <button class="af-btn af-btn-primary" @click="contentDialogOpen = false">{{ t("overview.contentDone") }}</button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.overview {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.toolbar-under {
  display: flex;
  justify-content: flex-end;
  margin-top: -8px;
}

.add-bar {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 10px 12px;
}
.add-bar .af-btn {
  flex-shrink: 0;
  white-space: nowrap;
}
.add-icon {
  width: 30px;
  height: 30px;
  border-radius: 9px;
  background: var(--af-accent-soft);
  color: var(--af-accent);
  display: flex;
  align-items: center;
  justify-content: center;
  font-weight: 700;
}
.add-input {
  flex: 1;
  /* flex items default to min-width:auto — a <input> then refuses to shrink
     below its intrinsic width and pushes the buttons out of the panel on
     narrow screens. */
  min-width: 0;
  border: 0;
  background: transparent;
  box-shadow: none;
  font-size: 14px;
}
.add-input:focus {
  box-shadow: none;
}

.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(112px, 1fr));
  gap: 10px;
}
.stat-card {
  padding: 12px 12px 10px;
  text-align: center;
  cursor: pointer;
  font-family: inherit;
  color: inherit;
  transition: border-color 0.15s ease, transform 0.15s ease;
}
.stat-card:hover {
  border-color: var(--af-accent);
  transform: translateY(-1px);
}
.stat-card:focus-visible {
  outline: 2px solid var(--af-accent);
  outline-offset: 2px;
}
.content-btn {
  display: inline-flex;
  align-items: center;
  gap: 7px;
}
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow-y: auto;
  padding: 72px 16px 32px;
  background: rgba(0, 0, 0, 0.36);
}
.content-dialog {
  width: min(430px, 100%);
  padding: 18px 20px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.content-title {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}
.content-hint {
  margin: -4px 0 0;
  font-size: 11.5px;
  line-height: 1.5;
  color: var(--af-text-dim);
}
.content-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.content-group-label {
  font-size: 11px;
  font-weight: 650;
  color: var(--af-text-dim);
  text-transform: uppercase;
  letter-spacing: 0.06em;
}
.content-group .check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
  color: var(--af-text);
  cursor: pointer;
  padding: 3px 2px;
  user-select: none;
}
.content-group .check-row input:disabled {
  opacity: 0.45;
}
.content-group .check-row.disabled {
  opacity: 0.6;
  cursor: default;
}
.content-actions {
  display: flex;
  justify-content: flex-end;
  border-top: 1px solid var(--af-border);
  padding-top: 12px;
}
.stat-value {
  font-size: 22px;
  font-weight: 750;
  font-variant-numeric: tabular-nums;
}
.stat-value.mono {
  font-size: 15px;
  line-height: 1.5;
}
.stat-label {
  margin-top: 2px;
  font-size: 11px;
  color: var(--af-text-dim);
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
.section-head-actions {
  display: flex;
  align-items: center;
  gap: 14px;
}
.show-paused-toggle {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  font-size: 12px;
  color: var(--af-text-dim);
  cursor: pointer;
  user-select: none;
}
.show-paused-toggle input:disabled {
  opacity: 0.45;
}
.section-head h2 {
  margin: 0;
  font-size: 14px;
  font-weight: 650;
  color: var(--af-text-dim);
  letter-spacing: 0.4px;
}

.task-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.empty {
  text-align: center;
  padding: 42px 0 30px;
  color: var(--af-text-dim);
}
.empty-art {
  font-size: 40px;
  opacity: 0.35;
}
.dim {
  font-size: 12px;
  opacity: 0.75;
}

@media (max-width: 1100px) {
  .stat-grid {
    grid-template-columns: repeat(4, minmax(0, 1fr));
  }
}
@media (max-width: 767px) {
  .add-bar {
    gap: 8px;
    padding: 10px;
  }
}

/* Phone: stack into two rows — full-width input on one row, equal-width
   buttons on the next. Prevents overflow/clipping of 开始下载 / Advanced… */
@media (max-width: 560px) {
  .add-bar {
    flex-wrap: wrap;
    row-gap: 8px;
  }
  .add-icon {
    display: none;
  }
  .add-input {
    flex: 1 1 100%;
    order: 1;
    /* keep a comfortable tap target and visible ring on small screens */
    min-height: 40px;
    padding-left: 4px;
  }
  .add-bar .af-btn {
    order: 2;
    flex: 1 1 0;
    min-width: 0;
    padding-left: 8px;
    padding-right: 8px;
  }
}

</style>
