<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { api, type ProxyEntry } from "../lib/api";
import { showNotice } from "../lib/notice";
import { useI18n } from "../lib/i18n";
import {
  EMPTY_ADVANCED,
  buildAdvancedPayload,
  certificateAuthError,
  filenameFromUrl,
  isSshLikeUrl,
  type AdvancedOptions,
} from "../lib/task-options";
import AdvancedDownloadFields from "./AdvancedDownloadFields.vue";

const props = defineProps<{ open: boolean; initialUrl?: string }>();
const emit = defineEmits<{ close: []; added: [filename: string] }>();

const { t } = useI18n();

const url = ref("");
const filename = ref("");
/** True once the user edits the filename box — stops the URL→filename auto-fill. */
const filenameTouched = ref(false);
const advanced = reactive<AdvancedOptions>({ ...EMPTY_ADVANCED });
const proxies = ref<ProxyEntry[]>([]);
const submitting = ref(false);

/** ssh/sftp URLs can use certificate authentication. */
const sshLike = computed(() => isSshLikeUrl(url.value));

function onUrlInput() {
  if (filenameTouched.value) return;
  const derived = filenameFromUrl(url.value);
  // Only overwrite with something meaningful; never blank the field out.
  if (derived) filename.value = derived;
}

function onFilenameInput() {
  // Clearing the box re-enables auto-fill from the URL.
  filenameTouched.value = filename.value.trim().length > 0;
}

onMounted(async () => {
  try {
    proxies.value = await api.listProxies();
  } catch {
    proxies.value = [];
  }
});

watch(
  () => props.open,
  (open) => {
    if (open) {
      url.value = props.initialUrl ?? "";
      filename.value = filenameFromUrl(props.initialUrl ?? "");
      filenameTouched.value = false;
      Object.assign(advanced, EMPTY_ADVANCED);
      submitting.value = false;
    }
  },
);

async function submit() {
  const trimmed = url.value.trim();
  if (!trimmed || submitting.value) return;
  const certError = certificateAuthError(advanced);
  if (certError) {
    showNotice(t(certError));
    return;
  }
  submitting.value = true;
  try {
    const task = await api.addTask({
      url: trimmed,
      ...(filename.value.trim() ? { filename: filename.value.trim() } : {}),
      ...buildAdvancedPayload(advanced, proxies.value),
    });
    emit("added", task.filename);
    emit("close");
  } catch (err) {
    // Inline errors just echo the daemon; surface them as a toast instead.
    showNotice(err instanceof Error ? err.message : String(err));
  } finally {
    submitting.value = false;
  }
}

</script>

<template>
  <Transition name="dialog">
    <div v-if="open" class="dialog-backdrop" @click.self="emit('close')">
    <div class="dialog af-panel" role="dialog" aria-modal="true">
      <h2 class="dialog-title">{{ t("dialog.addTitle") }}</h2>

      <label class="field-label">URL</label>
      <input
        v-model="url"
        class="af-input"
        type="text"
        spellcheck="false"
        autocomplete="off"
        :placeholder="t('overview.addPlaceholder')"
        @keydown.enter="submit"
        @input="onUrlInput"
      />

      <label class="field-label">{{ t("dialog.filename") }}</label>
      <input v-model="filename" class="af-input" type="text" spellcheck="false" placeholder="file.zip" @input="onFilenameInput" />

      <AdvancedDownloadFields v-model="advanced" :proxies="proxies" :ssh-like="sshLike" />

      <div class="dialog-actions">
        <button class="af-btn" :disabled="submitting" @click="emit('close')">
          {{ t("dialog.cancel") }}
        </button>
        <button class="af-btn af-btn-primary" :disabled="submitting || !url.trim()" @click="submit">
          {{ submitting ? t("dialog.adding") : t("dialog.add") }}
        </button>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.dialog-backdrop {
  position: fixed;
  inset: 0;
  z-index: 40;
  display: flex;
  align-items: flex-start;
  justify-content: center;
  overflow-y: auto;
  padding: 48px 16px 32px;
  background: rgba(0, 0, 0, 0.36);
}
.dialog {
  width: min(560px, 100%);
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 18px 20px;
}
.dialog-title {
  margin: 0 0 4px;
  font-size: 15px;
  font-weight: 650;
}
.field-label {
  font-size: 11px;
  color: var(--af-text-dim);
  margin-top: 2px;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 6px;
}
</style>
