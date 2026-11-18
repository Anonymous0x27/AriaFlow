<script setup lang="ts">
import { computed, onMounted, reactive, ref, watch } from "vue";
import { api, type BatchAddResult, type ProxyEntry } from "../lib/api";
import { showNotice } from "../lib/notice";
import { useI18n } from "../lib/i18n";
import {
  EMPTY_ADVANCED,
  buildAdvancedPayload,
  certificateAuthError,
  isSshLikeUrl,
  parseUrlList,
  type AdvancedOptions,
} from "../lib/task-options";
import AdvancedDownloadFields from "./AdvancedDownloadFields.vue";

const props = defineProps<{ open: boolean }>();
const emit = defineEmits<{ close: []; added: [count: number] }>();

const { t } = useI18n();

const urlsText = ref("");
const advanced = reactive<AdvancedOptions>({ ...EMPTY_ADVANCED });
const proxies = ref<ProxyEntry[]>([]);
const submitting = ref(false);
const result = ref<BatchAddResult | null>(null);

const urls = computed(() => parseUrlList(urlsText.value));
/** ssh/sftp URLs can use certificate authentication (one setting for the batch). */
const sshLike = computed(() => urls.value.some(isSshLikeUrl));

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
      urlsText.value = "";
      Object.assign(advanced, EMPTY_ADVANCED);
      submitting.value = false;
      result.value = null;
    }
  },
);

async function submit() {
  const list = urls.value;
  if (list.length === 0 || submitting.value) {
    showNotice(t("dialog.batchEmpty"));
    return;
  }
  const certError = certificateAuthError(advanced);
  if (certError) {
    showNotice(t(certError));
    return;
  }
  submitting.value = true;
  try {
    const advancedPayload = buildAdvancedPayload(advanced, proxies.value);
    result.value = await api.batchAdd(list.map((url) => ({ url, ...advancedPayload })));
    emit("added", result.value.affected);
  } catch (err) {
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
      <h2 class="dialog-title">{{ t("dialog.batchTitle") }}</h2>

      <label class="field-label">{{ t("dialog.batchUrls") }}</label>
      <textarea
        v-model="urlsText"
        class="af-input mono urls"
        rows="6"
        spellcheck="false"
        placeholder="https://example.com/a.zip&#10;https://example.com/b.zip&#10;https://example.com/c.zip"
      ></textarea>
      <p class="hint">{{ t("dialog.batchHint", { count: urls.length }) }}</p>

      <AdvancedDownloadFields v-model="advanced" :proxies="proxies" :ssh-like="sshLike" />

      <div v-if="result" class="batch-result" :class="{ hasErrors: result.errors?.length }">
        <p class="batch-summary">{{ t("dialog.batchResult", { affected: result.affected }) }}</p>
        <ul v-if="result.errors?.length" class="batch-errors">
          <li v-for="item in result.errors" :key="`${item.index}|${item.url}`">
            <code>{{ item.url }}</code>
            <span>{{ item.message }}</span>
          </li>
        </ul>
      </div>

      <div class="dialog-actions">
        <button class="af-btn" :disabled="submitting" @click="emit('close')">
          {{ result ? t("dialog.close") : t("dialog.cancel") }}
        </button>
        <button
          v-if="!result"
          class="af-btn af-btn-primary"
          :disabled="submitting || urls.length === 0"
          @click="submit"
        >
          {{ submitting ? t("dialog.adding") : t("dialog.batchSubmit", { count: urls.length }) }}
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
  width: min(580px, 100%);
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
.mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
textarea.urls {
  resize: none;
  width: 100%;
  height: 15vh;
  min-height: 96px;
  max-height: 240px;
  overflow-y: auto;
}
.hint {
  color: var(--af-text-dim);
  font-size: 11px;
  margin: -2px 0 0;
}
.batch-result {
  border: 1px solid rgba(52, 199, 89, 0.4);
  background: rgba(52, 199, 89, 0.09);
  border-radius: var(--af-radius-sm);
  padding: 8px 12px;
  font-size: 12px;
}
.batch-result.hasErrors {
  border-color: rgba(255, 159, 10, 0.45);
  background: rgba(255, 159, 10, 0.1);
}
.batch-summary {
  margin: 0;
  font-weight: 600;
}
.batch-errors {
  margin: 6px 0 0;
  padding-left: 18px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}
.batch-errors code {
  font-size: 10px;
  color: var(--af-text-dim);
  display: block;
  word-break: break-all;
}
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 4px;
}
</style>
