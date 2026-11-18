<script setup lang="ts">
import { computed } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "../lib/i18n";

const { t, locale, setLocale } = useI18n();

interface NavItem {
  to: string;
  labelKey: string;
  icon: string;
}

const groups = computed<{ title?: string; items: NavItem[] }[]>(() => [
  {
    items: [{ to: "/", labelKey: "nav.overview", icon: "◎" }],
  },
  {
    title: t("nav.group.tasks"),
    items: [
      { to: "/downloads", labelKey: "nav.downloads", icon: "▤" },
      { to: "/downloads/active", labelKey: "nav.active", icon: "↓" },
      { to: "/downloads/queued", labelKey: "nav.queued", icon: "◷" },
      { to: "/downloads/paused", labelKey: "nav.paused", icon: "⏸" },
      { to: "/downloads/completed", labelKey: "nav.completed", icon: "✓" },
      { to: "/downloads/failed", labelKey: "nav.failed", icon: "!" },
    ],
  },
  {
    title: t("nav.group.organization"),
    items: [
      { to: "/history", labelKey: "nav.history", icon: "↺" },
      { to: "/queue", labelKey: "nav.queue", icon: "☰" },
      { to: "/browser", labelKey: "nav.browser", icon: "⌘" },
    ],
  },
  {
    title: t("nav.group.system"),
    items: [
      { to: "/settings", labelKey: "nav.settings", icon: "⚙" },
      { to: "/about", labelKey: "nav.about", icon: "ℹ" },
    ],
  },
]);

// `open` toggles the off-canvas drawer on phone-sized screens (App.vue owns the
// transform); this component only needs the type. Wider viewports ignore it.
defineProps<{ open?: boolean }>();
const emit = defineEmits<{ close: [] }>();

const route = useRoute();
const router = useRouter();

function isActive(to: string): boolean {
  if (to === "/") return route.path === "/";
  return route.path === to || route.path.startsWith(`${to}/`);
}

function go(to: string) {
  void router.push(to);
  emit("close");
}
</script>

<template>
  <aside class="sidebar">
    <div class="brand">
      <div class="brand-mark">↓</div>
      <div class="brand-text">
        <div class="brand-name">AriaFlow</div>
        <div class="brand-sub">{{ t("nav.brandSub") }}</div>
      </div>
      <button class="drawer-close" :aria-label="t('app.menuToggle')" @click="emit('close')">
        <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" aria-hidden="true">
          <path d="M6 6l12 12M18 6L6 18" />
        </svg>
      </button>
    </div>

    <nav class="nav scroll-y">
      <template v-for="(group, gi) in groups" :key="gi">
        <div v-if="group.title" class="nav-group-title">{{ group.title }}</div>
        <button
          v-for="item in group.items"
          :key="item.to"
          class="nav-item"
          :class="{ active: isActive(item.to) }"
          @click="go(item.to)"
        >
          <span class="nav-icon">{{ item.icon }}</span>
          <span class="nav-label">{{ t(item.labelKey) }}</span>
        </button>
      </template>
    </nav>

    <div class="sidebar-foot">
      <div class="foot-line">AriaFlow 0.1.0</div>
      <div class="foot-line dim">{{ t("nav.footDaemon") }}</div>
      <button class="lang-toggle" @click="setLocale(locale.locale === 'zh-CN' ? 'en' : 'zh-CN')">
        {{ locale.locale === "zh-CN" ? "EN" : "中" }}
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--af-sidebar);
  backdrop-filter: blur(var(--af-blur)) saturate(1.8);
  -webkit-backdrop-filter: blur(var(--af-blur)) saturate(1.8);
  padding: 18px 12px 12px;
  user-select: none;
}

.brand {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 2px 8px 16px;
}

.drawer-close {
  display: none;
  margin-left: auto;
  border: 0;
  background: transparent;
  color: var(--af-text-dim);
  border-radius: 8px;
  padding: 6px;
  cursor: pointer;
}
.drawer-close:hover {
  color: var(--af-text);
  background: rgba(127, 127, 127, 0.12);
}

.brand-mark {
  width: 38px;
  height: 38px;
  border-radius: 11px;
  background: linear-gradient(160deg, #3fa2ff, var(--af-accent) 60%, #0060df);
  color: #fff;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 20px;
  font-weight: 700;
  box-shadow: var(--af-shadow-sm);
}

.brand-name {
  font-size: 16px;
  font-weight: 700;
  letter-spacing: 0.3px;
}
.brand-sub {
  font-size: 11px;
  color: var(--af-text-dim);
}

.nav {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.nav-group-title {
  font-size: 11px;
  font-weight: 600;
  color: var(--af-text-dim);
  letter-spacing: 0.6px;
  padding: 12px 10px 4px;
}

.nav-item {
  display: flex;
  align-items: center;
  gap: 10px;
  border: 0;
  background: transparent;
  color: var(--af-text);
  padding: 7px 10px;
  border-radius: var(--af-radius-sm);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
  transition: background 0.15s ease;
}
.nav-item:hover {
  background: rgba(127, 127, 127, 0.12);
}
.nav-item.active {
  background: var(--af-accent-soft);
  color: var(--af-accent);
  font-weight: 600;
}

.nav-icon {
  width: 18px;
  text-align: center;
  opacity: 0.9;
}

.sidebar-foot {
  padding: 10px 10px 2px;
  border-top: 1px solid var(--af-border);
  font-size: 11px;
}
.foot-line.dim {
  color: var(--af-text-dim);
}
.lang-toggle {
  margin-top: 6px;
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text-dim);
  border-radius: 5px;
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.5px;
  padding: 2px 8px;
  cursor: pointer;
}
.lang-toggle:hover {
  color: var(--af-accent);
  border-color: var(--af-accent);
}

/* Tablet: 76px icon rail — labels hidden, icons centered */
@media (max-width: 1199px) and (min-width: 768px) {
  .sidebar {
    padding: 14px 8px 10px;
  }
  .brand {
    justify-content: center;
    padding: 2px 0 14px;
  }
  .brand-text,
  .nav-group-title,
  .nav-label,
  .sidebar-foot {
    display: none;
  }
  .nav-item {
    justify-content: center;
    padding: 10px 0;
  }
  .nav-icon {
    width: auto;
    font-size: 17px;
  }
}

/* Phone: off-canvas drawer (App.vue positions it) — full labels, close button */
@media (max-width: 767px) {
  .drawer-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
}
</style>
