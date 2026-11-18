<script setup lang="ts">
import { computed, onMounted } from "vue";
import { useI18n } from "../lib/i18n";
import { useDownloadsStore } from "../stores/downloads";

const { t } = useI18n();
const store = useDownloadsStore();

const version = computed(() => store.daemonVersion || "…");

onMounted(() => {
  if (!store.connected) void store.refresh();
});
</script>

<template>
  <div class="about-view">
    <section class="af-panel hero">
      <div class="logo">A</div>
      <h1 class="title">AriaFlow</h1>
      <p class="tagline">{{ t("about.tagline") }}</p>
    </section>

    <section class="af-panel facts">
      <div class="fact-row">
        <span class="fact-label">{{ t("about.version") }}</span>
        <span class="mono">{{ version }}</span>
      </div>
      <div class="fact-row">
        <span class="fact-label">{{ t("about.license") }}</span>
        <span>{{ t("about.licenseValue") }}</span>
      </div>
      <div class="fact-row">
        <span class="fact-label">{{ t("about.stack") }}</span>
        <span>{{ t("about.stackValue") }}</span>
      </div>
    </section>

    <section class="af-panel section">
      <h2 class="section-title">{{ t("about.components") }}</h2>
      <ul class="components">
        <li>{{ t("about.componentDaemon") }}</li>
        <li>{{ t("about.componentCli") }}</li>
        <li>{{ t("about.componentDesktop") }}</li>
        <li>{{ t("about.componentBrowser") }}</li>
      </ul>
    </section>
  </div>
</template>

<style scoped>
.about-view {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 640px;
}
.hero {
  text-align: center;
  padding: 36px 16px 28px;
}
.logo {
  width: 72px;
  height: 72px;
  border-radius: 18px;
  margin: 0 auto 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 34px;
  font-weight: 700;
  color: #fff;
  background: linear-gradient(145deg, var(--af-accent), #5e5ce6);
}
.title {
  font-size: 24px;
  font-weight: 700;
  margin: 0;
}
.tagline {
  color: var(--af-text-dim);
  font-size: 13px;
  margin: 6px 0 0;
}
.facts {
  padding: 8px 18px;
}
.fact-row {
  display: flex;
  justify-content: space-between;
  gap: 24px;
  padding: 10px 0;
  border-bottom: 1px solid var(--af-border);
  font-size: 12px;
}
.fact-row:last-child {
  border-bottom: 0;
}
.fact-label {
  color: var(--af-text-dim);
  flex: none;
}
.mono {
  font-family: var(--af-font-mono);
}
.section {
  padding: 16px 18px;
}
.section-title {
  font-size: 13px;
  font-weight: 600;
  margin: 0 0 10px;
}
.components {
  margin: 0;
  padding-left: 18px;
  color: var(--af-text-dim);
  font-size: 12px;
  line-height: 1.9;
}
</style>
