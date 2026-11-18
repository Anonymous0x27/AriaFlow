<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { api, type ProxyEntry, type SshCertificateEntry } from "../lib/api";
import { useI18n } from "../lib/i18n";
import type { AdvancedOptions } from "../lib/task-options";
import DirPickerDialog from "./DirPickerDialog.vue";
import AriaDateTimePicker from "./AriaDateTimePicker.vue";
import AriaSelect from "./AriaSelect.vue";

const props = withDefaults(
  defineProps<{ proxies: ProxyEntry[]; sshLike?: boolean }>(),
  { sshLike: false },
);
const model = defineModel<AdvancedOptions>({ required: true });
const { t } = useI18n();
const dirPickerOpen = ref(false);
/** Global download dir shown as the save-dir placeholder (empty = daemon default). */
const globalDownloadDir = ref("");
/** Global User-Agent shown as the UA placeholder (empty box = follow global). */
const globalUserAgent = ref("");
/** Managed User-Agent list from settings; drives the themed picker. */
const userAgents = ref<string[]>([]);
/** Throwaway value of the AriaSelect picker (always reset after a pick). */
const uaPreset = ref("");
const certificates = ref<SshCertificateEntry[]>([]);
const userAgentOptions = computed(() =>
  userAgents.value.map((ua) => ({ value: ua, label: ua })),
);
watch(uaPreset, (value) => {
  // The picker is a "fill the input" action: writing into the model, then reset.
  if (!value) return;
  model.value.userAgent = value;
  uaPreset.value = "";
});
async function loadCertificates() {
  try {
    certificates.value = await api.listSshCertificates();
  } catch {
    certificates.value = [];
  }
}
onMounted(async () => {
  try {
    const settings = await api.settings();
    globalDownloadDir.value = settings.downloadDir ?? "";
    globalUserAgent.value = settings.userAgent ?? "";
    userAgents.value = [...(settings.userAgents ?? [])];
  } catch {
    globalDownloadDir.value = "";
    globalUserAgent.value = "";
    userAgents.value = [];
  }
  if (props.sshLike) void loadCertificates();
});
// Certificates are managed in Settings → they may appear after mount, so
// (re)load whenever the ssh/sftp context or the certificate selection changes.
watch(
  () => props.sshLike,
  (isSsh) => {
    if (isSsh) void loadCertificates();
  },
);
watch(
  () => model.value.authType,
  (authType) => {
    if (authType === "certificate") void loadCertificates();
  },
);
const saveDirPlaceholder = computed(() => globalDownloadDir.value || "/path/to/folder");
const userAgentPlaceholder = computed(() => globalUserAgent.value || "AriaFlow/1.0");
const userAgentHint = computed(() =>
  globalUserAgent.value
    ? t("dialog.userAgentInheritHint", { value: globalUserAgent.value })
    : t("dialog.userAgentInheritHintNoValue"),
);
const authOptions = computed(() => {
  const options = [
    { value: "none", label: t("dialog.authNone") },
    { value: "basic", label: t("dialog.authBasic") },
    { value: "bearer", label: t("dialog.authBearer") },
  ];
  // Certificate auth only makes sense for ssh/sftp transfers.
  if (props.sshLike) options.push({ value: "certificate", label: t("dialog.authCertificate") });
  return options;
});
const certificateOptions = computed(() => [
  { value: "", label: t("dialog.certificatePlaceholder") },
  ...certificates.value.map((cert) => ({
    value: cert.id,
    label: `${cert.name} · ${cert.algorithm}`,
  })),
]);
const certificateHint = computed(() =>
  certificates.value.length > 0 ? t("dialog.certificateHint") : t("dialog.certificateEmpty"),
);
const proxyOptions = computed(() => [
  { value: "", label: t("dialog.proxyDefault") },
  ...props.proxies.map((proxy) => ({ value: proxy.id, label: `${proxy.name} · ${proxy.kind}` })),
]);
const folderIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true">
    <path d="M3.5 6.8A2.3 2.3 0 0 1 5.8 4.5h3.7l2 2.3h6.7a2.3 2.3 0 0 1 2.3 2.3v8.2a2.3 2.3 0 0 1-2.3 2.3H5.8a2.3 2.3 0 0 1-2.3-2.3V6.8Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/>
  </svg>`;

function onDirPicked(path: string) {
  model.value.saveDir = path;
  dirPickerOpen.value = false;
}
</script>

<template>
  <div class="adv-fields">
    <div class="field full">
      <label class="field-label">{{ t("dialog.saveDir") }}</label>
      <div class="save-dir-row">
        <input v-model="model.saveDir" class="af-input" type="text" spellcheck="false" :placeholder="saveDirPlaceholder" :title="t('dialog.saveDirInheritHint')" />
        <button class="af-btn browse-btn" type="button" :title="t('picker.browse')" :aria-label="t('picker.browse')" @click="dirPickerOpen = true">
          <span class="browse-icon" v-html="folderIcon" />{{ t("picker.browse") }}
        </button>
      </div>
    </div>

    <div class="field full">
      <label class="field-label">{{ t("dialog.userAgent") }}</label>
      <div class="user-agent-row">
        <input
          v-model="model.userAgent"
          class="af-input mono"
          type="text"
          spellcheck="false"
          autocomplete="off"
          :placeholder="userAgentPlaceholder"
          :title="userAgentHint"
        />
        <AriaSelect
          v-if="userAgentOptions.length"
          v-model="uaPreset"
          class="ua-preset"
          :options="userAgentOptions"
          :placeholder="t('dialog.userAgentPick')"
          placement="up"
        />
      </div>
    </div>

    <div class="field full">
      <label class="field-label">{{ t("dialog.headers") }}</label>
      <textarea
        v-model="model.headersText"
        class="af-input mono"
        rows="2"
        spellcheck="false"
        placeholder="Referer: https://example.com/&#10;Cookie: session=abc"
      ></textarea>
    </div>

    <div class="field">
      <label class="field-label">{{ t("dialog.auth") }}</label>
      <AriaSelect v-model="model.authType" :options="authOptions" />
    </div>

    <div class="field">
      <label class="field-label">{{ t("dialog.proxy") }}</label>
      <AriaSelect v-model="model.proxyId" :options="proxyOptions" placement="down" />
    </div>

    <template v-if="model.authType === 'basic'">
      <div class="field">
        <label class="field-label">{{ t("dialog.username") }}</label>
        <input v-model="model.username" class="af-input" type="text" spellcheck="false" autocomplete="off" />
      </div>
      <div class="field">
        <label class="field-label">{{ t("dialog.password") }}</label>
        <input v-model="model.password" class="af-input" type="password" autocomplete="new-password" />
      </div>
    </template>
    <template v-else-if="model.authType === 'bearer'">
      <div class="field full">
        <label class="field-label">{{ t("dialog.token") }}</label>
        <input v-model="model.token" class="af-input mono" type="password" spellcheck="false" autocomplete="off" />
      </div>
    </template>
    <template v-else-if="model.authType === 'certificate'">
      <div class="field full">
        <label class="field-label">{{ t("dialog.certificate") }}</label>
        <AriaSelect v-model="model.certificateId" :options="certificateOptions" />
        <p class="field-hint">{{ certificateHint }}</p>
      </div>
    </template>

    <div class="field full row">
      <label class="check-row">
        <input v-model="model.startPaused" type="checkbox" />
        {{ t("dialog.startPaused") }}
      </label>
    </div>
    <div class="field">
      <label class="field-label">{{ t("dialog.limitRate") }}</label>
      <div class="input-with-unit">
        <input
          v-model="model.limitRateText"
          class="af-input"
          type="number"
          min="0"
          step="any"
          inputmode="decimal"
          :placeholder="t('dialog.limitRatePlaceholder')"
        />
        <span class="unit">KB/s</span>
      </div>
    </div>

    <div class="field">
      <label class="field-label">{{ t("dialog.scheduledStart") }}</label>
      <AriaDateTimePicker v-model="model.scheduledStart" placement="up" :placeholder="t('datePicker.placeholder')" />
    </div>
  </div>
  <DirPickerDialog
    :open="dirPickerOpen"
    :initial-path="model.saveDir"
    @close="dirPickerOpen = false"
    @select="onDirPicked"
  />
</template>

<style scoped>
.adv-fields {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 10px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}
.field.full {
  grid-column: 1 / -1;
}
.field.row {
  margin-top: -2px;
}
.field-label {
  font-size: 11px;
  color: var(--af-text-dim);
}
.mono {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
textarea.af-input {
  resize: none;
  width: 100%;
  height: 8.5vh;
  min-height: 52px;
  max-height: 160px;
  overflow-y: auto;
}
.input-with-unit {
  position: relative;
  display: flex;
  align-items: center;
}
.input-with-unit .af-input {
  flex: 1;
  padding-right: 44px;
}
.input-with-unit .unit {
  position: absolute;
  right: 11px;
  font-size: 11px;
  color: var(--af-text-dim);
  pointer-events: none;
}
.save-dir-row {
  display: flex;
  gap: 8px;
}
.user-agent-row {
  display: flex;
  gap: 8px;
  align-items: stretch;
}
.user-agent-row .af-input {
  flex: 1;
  min-width: 0;
}
.ua-preset {
  flex-shrink: 0;
  width: 168px;
}
.ua-preset :deep(.asel-value) {
  font-family: var(--af-font-mono);
  font-size: 11px;
}
.save-dir-row .af-input {
  flex: 1;
  min-width: 0;
}
.browse-btn {
  flex-shrink: 0;
}
.browse-icon {
  display: inline-flex;
}
.field-hint {
  font-size: 11px;
  color: var(--af-text-dim);
  margin: 2px 0 0;
}
.check-row {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--af-text-dim);
  cursor: pointer;
}
.check-row input {
  accent-color: var(--af-accent);
}
@media (max-width: 640px) {
  .adv-fields {
    grid-template-columns: 1fr;
  }
}
</style>
