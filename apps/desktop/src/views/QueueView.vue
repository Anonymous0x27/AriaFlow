<script setup lang="ts">
import { onMounted, computed } from "vue";
import { api, humanBytes } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useDownloadsStore } from "../stores/downloads";

const { t } = useI18n();
const store = useDownloadsStore();

// The queue is the set of not-yet-started tasks in scheduler order:
// priority descending, then enqueue time ascending.
const tasks = computed(() =>
  store.tasks
    .filter((task) => ["pending", "queued", "retrying", "waiting"].includes(task.status))
    .slice()
    .sort((a, b) => b.priority - a.priority || a.createdAt.localeCompare(b.createdAt)),
);

async function refresh() {
  // Reuse the polling store but scope it to queued-like tasks.
  store.filter = "queued";
  await store.refresh();
}

async function nudge(task: { id: string; priority: number }, delta: number) {
  await api.setPriority(task.id, task.priority + delta);
  await refresh();
}

async function pinToTop(task: { id: string; priority: number }) {
  const max = Math.max(0, ...tasks.value.map((t) => t.priority));
  if (task.priority < max + 1) {
    await api.setPriority(task.id, max + 1);
    await refresh();
  }
}

function createdLabel(iso: string): string {
  return iso.replace("T", " ").replace(/\+.*$/, "").slice(0, 16);
}

onMounted(() => refresh());
</script>

<template>
  <div class="queue-view">
    <p class="hint">{{ t("queue.hint") }}</p>

    <div v-if="tasks.length === 0" class="empty af-panel">
      <div class="empty-art">☰</div>
      <p>{{ t("queue.emptyTitle") }}</p>
      <p class="dim">{{ t("queue.emptyHint") }}</p>
    </div>

    <div v-else class="table-scroll">
    <table class="queue-table af-panel">
      <thead>
        <tr>
          <th>{{ t("queue.col.position") }}</th>
          <th>{{ t("queue.col.filename") }}</th>
          <th>{{ t("queue.col.priority") }}</th>
          <th>{{ t("queue.col.scheduled") }}</th>
          <th>{{ t("queue.col.created") }}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        <tr v-for="(task, index) in tasks" :key="task.id">
          <td class="position">{{ index + 1 }}</td>
          <td class="filename" :title="task.url">
            {{ task.filename }}
            <span v-if="task.totalSize != null" class="size dim">{{ humanBytes(task.totalSize) }}</span>
          </td>
          <td class="mono">{{ task.priority }}</td>
          <td>
            <span v-if="task.scheduledStartAt" class="scheduled-badge" :title="task.scheduledStartAt">
              {{ t("queue.scheduledBadge") }} {{ createdLabel(task.scheduledStartAt) }}
            </span>
            <span v-else class="dim">--</span>
          </td>
          <td class="mono dim">{{ createdLabel(task.createdAt) }}</td>
          <td class="actions">
            <button class="mini-btn" :title="t('queue.pinTop')" @click="pinToTop(task)">⇤</button>
            <button class="mini-btn" :title="t('queue.moveUp')" @click="nudge(task, 1)">▲</button>
            <button class="mini-btn" :title="t('queue.moveDown')" @click="nudge(task, -1)">▼</button>
          </td>
        </tr>
      </tbody>
    </table>
    </div>
  </div>
</template>

<style scoped>
.queue-view {
  display: flex;
  flex-direction: column;
  gap: 12px;
  max-width: 980px;
}
.hint {
  color: var(--af-text-dim);
  font-size: 12px;
  margin: 2px 2px 0;
}
.dim {
  color: var(--af-text-dim);
}
.queue-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 12px;
  padding: 4px 0;
}
.queue-table th,
.queue-table td {
  text-align: left;
  padding: 8px 12px;
  border-bottom: 1px solid var(--af-border);
}
.queue-table tr:last-child td {
  border-bottom: 0;
}
@media (max-width: 767px) {
  .queue-table {
    min-width: 640px;
  }
}
.queue-table th {
  color: var(--af-text-dim);
  font-size: 11px;
  font-weight: 600;
}
.position {
  color: var(--af-text-dim);
  font-variant-numeric: tabular-nums;
  width: 32px;
}
.filename {
  font-weight: 600;
  max-width: 420px;
}
.size {
  margin-left: 8px;
  font-weight: 400;
  font-size: 11px;
}
.mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
.scheduled-badge {
  font-size: 10px;
  border: 1px solid rgba(255, 159, 10, 0.5);
  color: var(--af-warning);
  border-radius: 4px;
  padding: 1px 6px;
}
.actions {
  display: flex;
  gap: 4px;
  white-space: nowrap;
}
.mini-btn {
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text-dim);
  border-radius: 5px;
  font-size: 11px;
  padding: 4px 8px;
  cursor: pointer;
}
.mini-btn:hover {
  color: var(--af-accent);
  border-color: var(--af-accent);
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
</style>
