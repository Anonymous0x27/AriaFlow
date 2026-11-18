<script setup lang="ts">
/** Completed-file save dialog, teleported to the viewport for task and history rows.
 * Web users can download to their browser device or copy on the daemon host;
 * desktop uses the daemon-side copy, including with IPC-only connections.
 */
import { computed, ref, watch } from "vue";
import { api, downloadHistoryFile, downloadTaskFile, isTauriEnv, type SaveAsTarget } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { showNotice } from "../lib/notice";
import DirPickerDialog from "./DirPickerDialog.vue";

const props = defineProps<{
  target: SaveAsTarget | null;
}>();
const emit = defineEmits<{ close: [] }>();

const { t } = useI18n();

const open = computed(() => props.target !== null);
const filename = ref("");
const destDir = ref("");
const overwrite = ref(false);
const pickerOpen = ref(false);
const saving = ref(false);
const error = ref<string | null>(null);
const browserDownloading = ref(false);
const desktop = isTauriEnv();

watch(
  () => props.target,
  (target) => {
    error.value = null;
    saving.value = false;
    overwrite.value = false;
    pickerOpen.value = false;
    filename.value = target?.filename ?? "";
    destDir.value = "";
    if (!target) return;
    // Best-effort starting point: the global download directory.
    void api
      .settings()
      .then((s) => {
        if (props.target === target && !destDir.value) destDir.value = s.downloadDir ?? "";
      })
      .catch(() => undefined);
  },
  { immediate: true },
);

function close() {
  if (!saving.value && !browserDownloading.value) emit("close");
}

async function downloadToDevice() {
  const target = props.target;
  if (!target || browserDownloading.value) return;
  browserDownloading.value = true;
  error.value = null;
  try {
    if (target.kind === "task") await downloadTaskFile(target);
    else await downloadHistoryFile(target);
    showNotice(t("saveAs.browserStarted"));
    emit("close");
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    browserDownloading.value = false;
  }
}

async function save() {
  const target = props.target;
  if (!target || saving.value) return;
  error.value = null;
  const dir = destDir.value.trim();
  if (!dir) {
    error.value = t("saveAs.destRequired");
    return;
  }
  const name = filename.value.trim();
  if (!name) {
    error.value = t("saveAs.filenameRequired");
    return;
  }
  saving.value = true;
  try {
    const result =
      target.kind === "task"
        ? await api.saveDownloadAs(target.id, dir, name, overwrite.value)
        : await api.saveHistoryAs(target.id, dir, name, overwrite.value);
    showNotice(t("saveAs.saved", { path: result.savedTo }));
    emit("close");
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    saving.value = false;
  }
}
</script>

<template>
  <Teleport to="body">
  <Transition name="dialog">
    <div v-if="open" class="dialog-backdrop" @click.self="close">
      <div
        class="dialog saveas-dialog af-panel"
        role="dialog"
        aria-modal="true"
        :aria-label="t('saveAs.title')"
        @keydown.escape="close"
      >
        <h2 class="saveas-title">{{ t("saveAs.title") }}</h2>
        <p class="saveas-target" :title="target?.filename">{{ target?.filename }}</p>

        <div v-if="!desktop" class="browser-save">
          <p>{{ t("saveAs.browserHint") }}</p>
          <button type="button" class="af-btn af-btn-primary" :disabled="browserDownloading || saving" @click="downloadToDevice">
            {{ browserDownloading ? t("saveAs.browserDownloading") : t("saveAs.browserDownload") }}
          </button>
        </div>
        <p class="copy-hint">{{ t("saveAs.serverHint") }}</p>
        <label class="field-label" for="saveas-dir">{{ t("saveAs.destDir") }}</label>
        <div class="dir-row">
          <input
            id="saveas-dir"
            v-model="destDir"
            class="af-input dir-input"
            type="text"
            spellcheck="false"
            autocomplete="off"
            :placeholder="t('saveAs.destPlaceholder')"
            @keydown.enter="save"
          />
          <button type="button" class="af-btn" @click="pickerOpen = true">{{ t("saveAs.chooseDir") }}</button>
        </div>

        <label class="field-label" for="saveas-name">{{ t("saveAs.filename") }}</label>
        <input
          id="saveas-name"
          v-model="filename"
          class="af-input mono"
          type="text"
          spellcheck="false"
          autocomplete="off"
          @keydown.enter="save"
        />

        <label class="overwrite-row">
          <input v-model="overwrite" type="checkbox" />
          <span>{{ t("saveAs.overwrite") }}</span>
        </label>

        <p v-if="error" class="error-text" role="alert">{{ error }}</p>

        <div class="dialog-actions">
          <button class="af-btn" :disabled="saving || browserDownloading" @click="close">{{ t("common.cancel") }}</button>
          <button class="af-btn af-btn-primary" :disabled="saving || browserDownloading" @click="save">
            {{ saving ? t("saveAs.saving") : t("saveAs.save") }}
          </button>
        </div>

      </div>
      <!-- Sibling of the panel (not nested), so the picker's own fixed backdrop
           stacks cleanly above this dialog (fp-overlay z-index 120 > 110). -->
      <DirPickerDialog
        :open="pickerOpen"
        :initial-path="destDir.trim() || undefined"
        @close="pickerOpen = false"
        @select="(path: string) => { destDir = path; pickerOpen = false; }"
      />
    </div>
  </Transition>
  </Teleport>
</template>

<style scoped>
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 110;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow-y: auto;
  padding: 72px 16px 32px;
  background: rgba(0, 0, 0, 0.4);
}
.saveas-dialog {
  width: min(480px, 100%);
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 18px 20px;
}
.saveas-title {
  margin: 0;
  font-size: 15px;
  font-weight: 650;
}
.saveas-target {
  margin: 0;
  font-size: 12px;
  font-weight: 600;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.browser-save { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
.browser-save p, .copy-hint { font-size: 12px; color: var(--af-text-dim); margin: 0; line-height: 1.5; }
.copy-hint { border-top: 1px solid var(--af-border); padding-top: 10px; }
.field-label {
  font-size: 11px;
  color: var(--af-text-dim);
  margin-bottom: -6px;
}
.dir-row {
  display: flex;
  gap: 8px;
}
.dir-input {
  flex: 1;
  min-width: 0;
}
.overwrite-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  cursor: pointer;
  user-select: none;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}
</style>
