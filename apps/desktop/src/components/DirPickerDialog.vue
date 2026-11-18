<script setup lang="ts">
/**
 * Folder picker backed by `ariaflowd` (`GET /api/v1/fs/list`).
 *
 * Works for the Tauri desktop shell (listing goes over the IPC proxy into the
 * daemon's pipe) and for the browser web UI served by the daemon itself.
 * The list is always the daemon host's filesystem — never the browser's.
 */
import { computed, nextTick, ref, watch } from "vue";
import { api, humanBytes, type FsEntry, type FsListResponse } from "../lib/api";
import { useI18n } from "../lib/i18n";

const props = defineProps<{
  open: boolean;
  /** Where to start browsing; falls back to the daemon user's home directory. */
  initialPath?: string;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "select", path: string): void;
}>();

const { t } = useI18n();

const listing = ref<FsListResponse | null>(null);
const loading = ref(false);
const error = ref<string | null>(null);
const showHidden = ref(false);
const activeIndex = ref(-1);
const panelEl = ref<HTMLElement | null>(null);

/** Directory rows only, in display order — keyboard navigation targets. */
const dirEntries = computed(() => (listing.value?.entries ?? []).filter((e) => e.kind === "dir"));

function pathSegments(path: string): { label: string; path: string }[] {
  if (!path) return [];
  // Windows drive root, e.g. "C:\"
  const drive = /^([a-zA-Z]:)[\\/]/.exec(path);
  const root = drive ? `${drive[1]}\\` : path.startsWith("/") ? "/" : "";
  if (!root) return [{ label: path, path }];
  const rest = path.slice(root.length).split(/[\\/]+/).filter(Boolean);
  const segments: { label: string; path: string }[] = [{ label: root === "/" ? "/" : root, path: root }];
  let acc = root === "/" ? "" : root.replace(/[\\/]+$/, "");
  for (const part of rest) {
    acc = root === "/" ? `${acc}/${part}` : `${acc}\\${part}`;
    segments.push({ label: part, path: acc });
  }
  return segments;
}

const crumbs = computed(() => pathSegments(listing.value?.path ?? ""));

async function load(path?: string) {
  loading.value = true;
  error.value = null;
  activeIndex.value = -1;
  try {
    listing.value = await api.fs.list(path, showHidden.value);
  } catch (err) {
    error.value = err instanceof Error ? err.message : String(err);
  } finally {
    loading.value = false;
  }
}

function openAt(path?: string) {
  void load(path);
}

function enterDir(entry: FsEntry) {
  if (entry.kind !== "dir") return;
  void load(entry.path);
}

function goUp() {
  const parent = listing.value?.parent;
  if (parent) void load(parent);
}

function goHome() {
  void load(undefined);
}

function toggleHidden() {
  showHidden.value = !showHidden.value;
  void load(listing.value?.path);
}

function choose() {
  const path = listing.value?.path;
  if (path) emit("select", path);
}

function onKeydown(event: KeyboardEvent) {
  if (!listing.value) return;
  const dirs = dirEntries.value;
  if (event.key === "ArrowDown") {
    event.preventDefault();
    activeIndex.value = Math.min(activeIndex.value + 1, dirs.length - 1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    activeIndex.value = Math.max(activeIndex.value - 1, 0);
  } else if (event.key === "Enter" && activeIndex.value >= 0 && dirs[activeIndex.value]) {
    event.preventDefault();
    enterDir(dirs[activeIndex.value]);
  } else if (event.key === "Backspace") {
    event.preventDefault();
    goUp();
  }
}

function onOverlayClick() {
  emit("close");
}

function formatModified(secs: number | null): string {
  if (!secs) return "—";
  const d = new Date(secs * 1000);
  if (Number.isNaN(d.getTime())) return "—";
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

watch(
  () => props.open,
  (open) => {
    if (!open) return;
    showHidden.value = false;
    const start = props.initialPath?.trim();
    void load(start ? start : undefined);
    nextTick(() => panelEl.value?.focus());
  },
);

const folderIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="18" height="18" aria-hidden="true">
    <path d="M3 6.5A2.5 2.5 0 0 1 5.5 4h4l2 2.5h7A2.5 2.5 0 0 1 21 9v8.5a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 17.5v-11Z" fill="var(--af-accent-soft)" stroke="var(--af-accent)" stroke-width="1.4" stroke-linejoin="round"/>
  </svg>`;
const fileIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="18" height="18" aria-hidden="true">
    <path d="M6 3.5h8L18.5 8v12a1.5 1.5 0 0 1-1.5 1.5H6A1.5 1.5 0 0 1 4.5 20V5A1.5 1.5 0 0 1 6 3.5Z" stroke="var(--af-text-dim)" stroke-width="1.3" stroke-linejoin="round"/>
    <path d="M14 3.5V8h4.5" stroke="var(--af-text-dim)" stroke-width="1.3" stroke-linejoin="round"/>
  </svg>`;
const otherIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="18" height="18" aria-hidden="true">
    <circle cx="12" cy="12" r="8" stroke="var(--af-text-dim)" stroke-width="1.3" stroke-dasharray="3 2.4"/>
  </svg>`;
const upIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="17" height="17" aria-hidden="true">
    <path d="M12 19V5m0 0-6.5 6.5M12 5l6.5 6.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
const homeIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="17" height="17" aria-hidden="true">
    <path d="M4 11.2 12 4l8 7.2M6.5 9.8V20h11V9.8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
const refreshIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="17" height="17" aria-hidden="true">
    <path d="M20 12a8 8 0 1 1-2.3-5.6M20 3v4h-4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>
  </svg>`;
const closeIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="18" height="18" aria-hidden="true">
    <path d="M6 6l12 12M18 6L6 18" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
  </svg>`;
const emptyIcon = `
  <svg viewBox="0 0 24 24" fill="none" width="44" height="44" aria-hidden="true">
    <path d="M3.5 7A2.5 2.5 0 0 1 6 4.5h3.6l2 2.4H18A2.5 2.5 0 0 1 20.5 9.4v7.1a2.5 2.5 0 0 1-2.5 2.5H6a2.5 2.5 0 0 1-2.5-2.5V7Z" stroke="var(--af-text-dim)" stroke-width="1.3" stroke-linejoin="round" opacity="0.65"/>
  </svg>`;

defineExpose({});
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
    <div v-if="open" class="fp-overlay" @click.self="onOverlayClick">
      <div
        ref="panelEl"
        class="fp-panel"
        role="dialog"
        aria-modal="true"
        :aria-label="t('picker.title')"
        tabindex="-1"
        @keydown="onKeydown"
        @keydown.escape.stop="emit('close')"
      >
        <!-- header -->
        <div class="fp-header">
          <span class="fp-badge" v-html="folderIcon" />
          <h3 class="fp-title">{{ t("picker.title") }}</h3>
          <button class="fp-icon-btn" type="button" :aria-label="t('picker.cancel')" @click="emit('close')" v-html="closeIcon" />
        </div>

        <!-- toolbar -->
        <div class="fp-toolbar">
          <button class="fp-icon-btn" type="button" :title="t('picker.up')" :aria-label="t('picker.up')" :disabled="!listing?.parent" @click="goUp" v-html="upIcon" />
          <button class="fp-icon-btn" type="button" :title="t('picker.home')" :aria-label="t('picker.home')" @click="goHome" v-html="homeIcon" />
          <button class="fp-icon-btn" type="button" :title="t('picker.refresh')" :aria-label="t('picker.refresh')" @click="openAt(listing?.path)" v-html="refreshIcon" />
          <nav class="fp-crumbs" aria-label="path">
            <template v-for="(crumb, i) in crumbs" :key="crumb.path + i">
              <span v-if="i > 0" class="fp-crumb-sep">›</span>
              <button type="button" class="fp-crumb" @click="openAt(crumb.path)">{{ crumb.label }}</button>
            </template>
          </nav>
          <button
            type="button"
            class="fp-toggle"
            :class="{ on: showHidden }"
            role="switch"
            :aria-checked="showHidden"
            :title="t('picker.hidden')"
            @click="toggleHidden"
          >
            <span class="fp-toggle-knob" />
            <span class="fp-toggle-label">{{ t("picker.hidden") }}</span>
          </button>
        </div>

        <!-- body -->
        <div class="fp-body">
          <div v-if="loading" class="fp-state">
            <span class="fp-spinner" aria-hidden="true" />
            <p>{{ t("picker.loading") }}</p>
          </div>
          <div v-else-if="error" class="fp-state">
            <svg viewBox="0 0 24 24" fill="none" width="40" height="40" aria-hidden="true">
              <path d="M12 3 2.5 20h19L12 3Z" stroke="var(--af-warning)" stroke-width="1.5" stroke-linejoin="round"/>
              <path d="M12 10v4.2M12 16.8h.01" stroke="var(--af-warning)" stroke-width="1.7" stroke-linecap="round"/>
            </svg>
            <p class="fp-error">{{ error }}</p>
            <div class="fp-state-actions">
              <button class="af-btn" type="button" @click="openAt(listing?.path)">{{ t("picker.retry") }}</button>
              <button class="af-btn" type="button" @click="goHome">{{ t("picker.homeFallback") }}</button>
            </div>
          </div>
          <template v-else-if="listing">
            <p v-if="listing.truncated" class="fp-truncated">{{ t("picker.truncated") }}</p>
            <div v-if="listing.entries.length === 0" class="fp-state">
              <span v-html="emptyIcon" />
              <p>{{ t("picker.empty") }}</p>
            </div>
            <div v-else class="fp-table" role="listbox" aria-label="entries">
              <div class="fp-row fp-row-head" aria-hidden="true">
                <span />
                <span>{{ t("picker.colName") }}</span>
                <span class="fp-cell-size">{{ t("picker.colSize") }}</span>
                <span class="fp-cell-date">{{ t("picker.colModified") }}</span>
              </div>
              <button
                v-for="entry in listing.entries"
                :key="entry.path"
                type="button"
                class="fp-row"
                :class="{ 'is-dir': entry.kind === 'dir', 'is-active': activeIndex >= 0 && dirEntries[activeIndex]?.path === entry.path }"
                role="option"
                :aria-selected="activeIndex >= 0 && dirEntries[activeIndex]?.path === entry.path"
                :title="entry.name"
                @dblclick="enterDir(entry)"
                @click="entry.kind === 'dir' ? (activeIndex = dirEntries.findIndex((d) => d.path === entry.path)) : undefined"
              >
                <span class="fp-cell-icon" v-html="entry.kind === 'dir' ? folderIcon : entry.kind === 'file' ? fileIcon : otherIcon" />
                <span class="fp-cell-name">{{ entry.name }}</span>
                <span class="fp-cell-size">{{ entry.kind === "file" ? humanBytes(entry.size) : "—" }}</span>
                <span class="fp-cell-date">{{ formatModified(entry.modifiedSecs) }}</span>
              </button>
            </div>
          </template>
        </div>

        <!-- footer -->
        <div class="fp-footer">
          <code class="fp-current" :title="listing?.path ?? ''">{{ listing?.path ?? "…" }}</code>
          <div class="fp-actions">
            <button class="af-btn" type="button" @click="emit('close')">{{ t("picker.cancel") }}</button>
            <button class="af-btn af-btn-primary" type="button" :disabled="!listing?.path" @click="choose">
              {{ t("picker.select") }}
            </button>
          </div>
        </div>
      </div>
    </div>
    </Transition>
  </Teleport>
</template>
