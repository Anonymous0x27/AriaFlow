<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { api } from "../lib/api";
import { useI18n } from "../lib/i18n";
import { useDownloadsStore } from "../stores/downloads";

const { t } = useI18n();
const store = useDownloadsStore();

const url = ref("");
const adding = ref(false);
const testResult = ref<{ filename: string } | null>(null);
const testError = ref<string | null>(null);

const connected = computed(() => store.connected);
const version = computed(() => store.daemonVersion);

async function checkDaemon() {
  await store.refresh();
}

async function takeover() {
  const trimmed = url.value.trim();
  if (!trimmed || adding.value) return;
  adding.value = true;
  testError.value = null;
  testResult.value = null;
  try {
    const task = await api.add(trimmed);
    testResult.value = { filename: task.filename };
    url.value = "";
    void store.refresh();
  } catch (err) {
    testError.value = err instanceof Error ? err.message : String(err);
  } finally {
    adding.value = false;
  }
}

onMounted(() => void checkDaemon());
</script>

<template>
  <div class="browser-view">
    <section class="af-panel section">
      <h2 class="section-title">{{ t("browser.howTitle") }}</h2>
      <p class="body">{{ t("browser.howBody") }}</p>
      <pre class="code">browser extension ─HTTP Web API→ ariaflowd (127.0.0.1:8787)</pre>
    </section>

    <section class="af-panel section">
      <h2 class="section-title">{{ t("browser.step1Title") }}</h2>
      <p class="body">{{ t("browser.chromiumExt") }}</p>
      <p class="body">{{ t("browser.firefoxExt") }}</p>
    </section>

    <section class="af-panel section">
      <h2 class="section-title">{{ t("browser.step2Title") }}</h2>
      <p class="body">{{ t("browser.step2Body") }}</p>
      <p class="hint">{{ t("browser.envNote") }}</p>
    </section>

    <section class="af-panel section">
      <h2 class="section-title">{{ t("browser.daemonTitle") }}</h2>
      <p class="body" :class="connected ? 'ok' : 'bad'">
        {{ connected ? t("browser.daemonOnline", { version }) : t("browser.daemonOffline") }}
      </p>
      <button class="af-btn" @click="checkDaemon">{{ t("settings.connection.test") }}</button>
    </section>

    <section class="af-panel section">
      <h2 class="section-title">{{ t("browser.testTitle") }}</h2>
      <p class="hint">{{ t("browser.testHint") }}</p>
      <div class="test-row">
        <input
          v-model="url"
          class="af-input"
          type="text"
          spellcheck="false"
          :placeholder="t('browser.testPlaceholder')"
          @keydown.enter="takeover"
        />
        <button class="af-btn primary" :disabled="adding || !url.trim()" @click="takeover">
          {{ t("browser.testButton") }}
        </button>
      </div>
      <p v-if="testResult" class="ok">{{ t("browser.testOk", { filename: testResult.filename }) }}</p>
      <p v-if="testError" class="error">{{ testError }}</p>
    </section>
  </div>
</template>

<style scoped>
.browser-view {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 860px;
}
.section {
  padding: 16px 18px;
}
.section-title {
  font-size: 13px;
  font-weight: 600;
  margin: 0 0 8px;
  color: var(--af-text);
}
.body {
  font-size: 12px;
  color: var(--af-text-dim);
  margin: 0 0 8px;
  line-height: 1.6;
}
.hint {
  font-size: 11px;
  color: var(--af-text-dim);
  margin: 6px 0 0;
  line-height: 1.6;
}
.code {
  font-family: var(--af-font-mono);
  font-size: 11px;
  background: var(--af-panel-solid);
  border: 1px solid var(--af-border);
  border-radius: 6px;
  padding: 8px 10px;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--af-text);
  margin: 8px 0;
}
.test-row {
  display: flex;
  gap: 8px;
  margin-top: 8px;
}
.test-row .af-input {
  flex: 1;
}
.ok {
  color: var(--af-success);
  font-size: 12px;
}
.bad {
  color: var(--af-danger);
}
.error {
  color: var(--af-danger);
  font-size: 12px;
}
</style>
