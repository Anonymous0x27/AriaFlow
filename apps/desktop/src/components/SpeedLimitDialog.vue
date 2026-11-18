<script setup lang="ts">
/**
 * Per-task speed-limit dialog, opened from the task context menu.
 * Three ways to leave a value behind:
 *   - preset chips (不限速 / 256K / 512K / 1M / 5M / 10M)
 *   - custom amount + unit (KB/s, MB/s)
 *   - "继承全局设置" — clears the per-task override (PUT null)
 */
import { computed, ref, watch } from "vue";
import { api, humanBytes, type DownloadTask } from "../lib/api";
import { useI18n } from "../lib/i18n";
import AriaSelect from "./AriaSelect.vue";

const props = defineProps<{
  task: DownloadTask | null;
}>();
const emit = defineEmits<{ close: []; saved: [restarted: boolean] }>();

const { t } = useI18n();

const open = computed(() => props.task !== null);
const inputValue = ref("");
const unit = ref<string>("KB");
const unitOptions = [
  { value: "KB", label: "KB/s" },
  { value: "MB", label: "MB/s" },
];
const globalDefault = ref<number | null>(null);
const saving = ref(false);
const error = ref<string | null>(null);

watch(
  () => props.task,
  (task) => {
    error.value = null;
    saving.value = false;
    if (!task) return;
    const current = task.limitRateBytesPerSec;
    if (current == null || current === 0) {
      inputValue.value = "";
      unit.value = "KB";
    } else if (current >= 1024 * 1024 && current % (1024 * 1024) === 0) {
      inputValue.value = String(current / (1024 * 1024));
      unit.value = "MB";
    } else {
      inputValue.value = String(Math.round(current / 1024));
      unit.value = "KB";
    }
    void api
      .settings()
      .then((s) => (globalDefault.value = s.perTaskSpeedLimit))
      .catch(() => (globalDefault.value = null));
  },
);

const currentLabel = computed(() => {
  const v = props.task?.limitRateBytesPerSec;
  if (v == null) {
    return globalDefault.value && globalDefault.value > 0
      ? t("speedLimit.inheritedWithDefault", { value: `${humanBytes(globalDefault.value)}/s` })
      : t("speedLimit.inherited");
  }
  return v === 0 ? t("speedLimit.unlimited") : `${humanBytes(v)}/s`;
});

const PRESETS: { label: string; kb: number }[] = [
  { label: "speedLimit.unlimited", kb: 0 },
  { label: "256 KB/s", kb: 256 },
  { label: "512 KB/s", kb: 512 },
  { label: "1 MB/s", kb: 1024 },
  { label: "5 MB/s", kb: 5 * 1024 },
  { label: "10 MB/s", kb: 10 * 1024 },
];

async function apply(bytesPerSec: number) {
  const task = props.task;
  if (!task || saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    const result = await api.setLimitRate(task.id, bytesPerSec);
    emit("saved", result.restarted);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    saving.value = false;
  }
}

async function applyCustom() {
  const raw = inputValue.value.trim();
  if (!raw) {
    error.value = t("speedLimit.invalidValue");
    return;
  }
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value) || value < 0) {
    error.value = t("speedLimit.invalidValue");
    return;
  }
  const factor = unit.value === "MB" ? 1024 * 1024 : 1024;
  await apply(Math.round(value * factor));
}

async function clearOverride() {
  const task = props.task;
  if (!task || saving.value) return;
  saving.value = true;
  error.value = null;
  try {
    const result = await api.setLimitRate(task.id, null);
    emit("saved", result.restarted);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    saving.value = false;
  }
}

function close() {
  emit("close");
}
</script>

<template>
  <Transition name="dialog">
    <div v-if="open" class="dialog-backdrop" @click.self="close">
    <div class="dialog limit-dialog af-panel" role="dialog" aria-modal="true" :aria-label="t('speedLimit.title')" @keydown.escape="close">
      <h2 class="limit-title">{{ t("speedLimit.title") }}</h2>
      <p v-if="task" class="limit-target" :title="task.filename">{{ task.filename }}</p>
      <p class="limit-current">{{ t("speedLimit.current", { value: currentLabel }) }}</p>

      <div class="preset-row">
        <button v-for="preset in PRESETS" :key="preset.label" class="preset-chip" :disabled="saving" @click="apply(preset.kb * 1024)">
          {{ preset.label.startsWith("speedLimit.") ? t(preset.label) : preset.label }}
        </button>
      </div>

      <label class="field-label">{{ t("speedLimit.custom") }}</label>
      <div class="custom-row">
        <input
          v-model="inputValue"
          class="af-input"
          type="number"
          min="0"
          step="any"
          inputmode="decimal"
          :placeholder="t('speedLimit.customPlaceholder')"
          @keydown.enter="applyCustom"
        />
        <AriaSelect v-model="unit" class="unit-select" placement="up" :options="unitOptions" />
        <button class="af-btn af-btn-primary" :disabled="saving || !inputValue.trim()" @click="applyCustom">
          {{ t("speedLimit.apply") }}
        </button>
      </div>

      <button class="link-inherit" :disabled="saving" @click="clearOverride">{{ t("speedLimit.inheritAction") }}</button>

      <p class="limit-hint">{{ t("speedLimit.hint") }}</p>
      <p v-if="error" class="error-text">{{ error }}</p>

      <div class="dialog-actions">
        <button class="af-btn" :disabled="saving" @click="close">{{ t("common.cancel") }}</button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 125;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow-y: auto;
  padding: 72px 16px 32px;
  background: rgba(0, 0, 0, 0.4);
}
.limit-dialog {
  width: min(440px, 100%);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 20px;
}
.limit-title {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}
.limit-target {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.limit-current {
  margin: 0;
  font-size: 12px;
  color: var(--af-text-dim);
}
.preset-row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.preset-chip {
  border: 1px solid var(--af-border);
  background: var(--af-bg);
  color: var(--af-text);
  border-radius: 999px;
  font-size: 11px;
  padding: 5px 11px;
  cursor: pointer;
}
.preset-chip:hover:not(:disabled) {
  border-color: var(--af-accent);
  color: var(--af-accent);
}
.field-label {
  font-size: 11px;
  color: var(--af-text-dim);
}
.custom-row {
  display: flex;
  gap: 8px;
}
.custom-row .af-input {
  min-width: 0;
}
.custom-row input.af-input {
  flex: 1;
}
.unit-select {
  width: 88px;
  flex: none;
}
.link-inherit {
  align-self: flex-start;
  border: 0;
  background: transparent;
  color: var(--af-accent);
  font-size: 12px;
  padding: 0;
  cursor: pointer;
}
.link-inherit:hover {
  text-decoration: underline;
}
.link-inherit:disabled,
.preset-chip:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}
.limit-hint {
  margin: 0;
  font-size: 11px;
  color: var(--af-text-dim);
}
.error-text {
  margin: 0;
  color: var(--af-danger);
  font-size: 12px;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 2px;
}
</style>
