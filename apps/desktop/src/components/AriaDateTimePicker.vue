<script setup lang="ts">
/**
 * AriaFlow-themed date-time picker — replaces the native <input type="datetime-local">,
 * whose calendar/clock UI ignores the app's theme and follows the browser/OS instead.
 * Everything here is plain Vue + CSS variables, so on desktop (WebKitGTK) and web it
 * renders exactly in the AriaFlow theme (light/dark/auto, custom accent).
 *
 * v-model contract: "" (unset) or "YYYY-MM-DDTHH:MM" in local wall time — identical to
 * datetime-local, so downstream payload code (`new Date(value)`) is unchanged.
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "../lib/i18n";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    placeholder?: string;
    /** Which way the popover opens; "up" for fields near the bottom of a dialog. */
    placement?: "up" | "down";
  }>(),
  { placement: "down" },
);

const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();

const { t, locale } = useI18n();
const open = ref(false);
const rootEl = ref<HTMLElement | null>(null);
const gridEl = ref<HTMLElement | null>(null);

interface Parts {
  y: number;
  m: number; // 0-based
  d: number;
  hh: number;
  mm: number;
}

function parse(value: string): Parts | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(String(value ?? "").trim());
  if (!match) return null;
  const p = { y: +match[1], m: +match[2] - 1, d: +match[3], hh: +match[4], mm: +match[5] };
  const probe = new Date(p.y, p.m, p.d, p.hh, p.mm);
  if (probe.getFullYear() !== p.y || probe.getMonth() !== p.m || probe.getDate() !== p.d) return null;
  if (p.hh > 23 || p.mm > 59) return null;
  return p;
}

const pad = (n: number) => String(n).padStart(2, "0");
const clampInt = (v: unknown, min: number, max: number, fallback: number) => {
  const n = Math.round(Number(v));
  return Number.isFinite(n) ? Math.min(max, Math.max(min, n)) : fallback;
};
function format(p: Parts): string {
  return `${p.y}-${pad(p.m + 1)}-${pad(p.d)}T${pad(p.hh)}:${pad(p.mm)}`;
}

const parts = computed(() => parse(props.modelValue));
const display = computed(() =>
  parts.value
    ? `${parts.value.y}-${pad(parts.value.m + 1)}-${pad(parts.value.d)} ${pad(parts.value.hh)}:${pad(parts.value.mm)}`
    : "",
);

const now = new Date();
const viewYear = ref(parts.value?.y ?? now.getFullYear());
const viewMonth = ref(parts.value?.m ?? now.getMonth());

watch(
  () => props.modelValue,
  (value) => {
    const p = parse(value);
    if (p) {
      viewYear.value = p.y;
      viewMonth.value = p.m;
    }
  },
);

const monthLabel = computed(() =>
  new Intl.DateTimeFormat(locale.locale || "en", { year: "numeric", month: "long" }).format(
    new Date(viewYear.value, viewMonth.value, 1),
  ),
);
const weekdayLabels = computed(() => {
  const fmt = new Intl.DateTimeFormat(locale.locale || "en", { weekday: "narrow" });
  // 2023-01-01 was a Sunday; week starts on Sunday.
  return Array.from({ length: 7 }, (_, i) => fmt.format(new Date(2023, 0, 1 + i)));
});

interface Cell {
  key: string;
  day: number;
  inMonth: boolean;
  isToday: boolean;
  isSelected: boolean;
  y: number;
  m: number;
  d: number;
}
const cells = computed<Cell[]>(() => {
  const firstWeekday = new Date(viewYear.value, viewMonth.value, 1).getDay();
  const today = new Date();
  const sel = parts.value;
  const out: Cell[] = [];
  for (let i = 0; i < 42; i++) {
    const date = new Date(viewYear.value, viewMonth.value, i - firstWeekday + 1);
    out.push({
      key: `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`,
      day: date.getDate(),
      inMonth: date.getMonth() === viewMonth.value,
      isToday: date.toDateString() === today.toDateString(),
      isSelected:
        !!sel && date.getFullYear() === sel.y && date.getMonth() === sel.m && date.getDate() === sel.d,
      y: date.getFullYear(),
      m: date.getMonth(),
      d: date.getDate(),
    });
  }
  return out;
});

const activeDate = () => {
  const p = parts.value;
  if (p) return { y: p.y, m: p.m, d: p.d, hh: p.hh, mm: p.mm };
  const d = new Date();
  return { y: d.getFullYear(), m: d.getMonth(), d: d.getDate(), hh: 9, mm: 0 };
};
const emitParts = (next: Parts) => emit("update:modelValue", format(next));
function pickCell(cell: Cell) {
  const base = activeDate();
  emitParts({ ...base, y: cell.y, m: cell.m, d: cell.d });
  if (!cell.inMonth) {
    viewYear.value = cell.y;
    viewMonth.value = cell.m;
  }
}
function prevMonth() {
  const d = new Date(viewYear.value, viewMonth.value - 1, 1);
  viewYear.value = d.getFullYear();
  viewMonth.value = d.getMonth();
}
function nextMonth() {
  const d = new Date(viewYear.value, viewMonth.value + 1, 1);
  viewYear.value = d.getFullYear();
  viewMonth.value = d.getMonth();
}
function setHour(v: unknown) {
  const base = activeDate();
  emitParts({ ...base, hh: clampInt(v, 0, 23, base.hh) });
}
function setMinute(v: unknown) {
  const base = activeDate();
  emitParts({ ...base, mm: clampInt(v, 0, 59, base.mm) });
}
function pickToday() {
  const d = new Date();
  const base = activeDate();
  viewYear.value = d.getFullYear();
  viewMonth.value = d.getMonth();
  emitParts({ ...base, y: d.getFullYear(), m: d.getMonth(), d: d.getDate() });
}
function clear() {
  emit("update:modelValue", "");
}
function toggle() {
  open.value = !open.value;
}
function close() {
  open.value = false;
}

const hourValue = computed(() => parts.value?.hh ?? 9);
const minuteValue = computed(() => parts.value?.mm ?? 0);

/** Arrow-key navigation across the 6x7 grid; Enter/Space clicks the focused day (native button). */
function onGridKeydown(event: KeyboardEvent) {
  const keys = ["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"];
  if (!keys.includes(event.key)) return;
  const grid = gridEl.value;
  if (!grid) return;
  const buttons = Array.from(grid.querySelectorAll<HTMLButtonElement>("button[data-day]"));
  const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
  if (index === -1) return;
  event.preventDefault();
  const delta = event.key === "ArrowLeft" ? -1 : event.key === "ArrowRight" ? 1 : event.key === "ArrowUp" ? -7 : 7;
  const next = buttons[Math.min(buttons.length - 1, Math.max(0, index + delta))];
  next?.focus();
}

function onDocumentPointerDown(event: PointerEvent) {
  if (open.value && rootEl.value && !rootEl.value.contains(event.target as Node)) {
    open.value = false;
  }
}
if (typeof document !== "undefined") {
  document.addEventListener("pointerdown", onDocumentPointerDown, true);
}
onBeforeUnmount(() => {
  if (typeof document !== "undefined") {
    document.removeEventListener("pointerdown", onDocumentPointerDown, true);
  }
});

const calendarIcon = `<svg viewBox="0 0 24 24" fill="none" width="15" height="15" aria-hidden="true"><rect x="3.5" y="5" width="17" height="16" rx="2.4" stroke="currentColor" stroke-width="1.6"/><path d="M3.5 9.6h17M8 3v4M16 3v4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`;
const chevronLeft = `<svg viewBox="0 0 24 24" fill="none" width="14" height="14" aria-hidden="true"><path d="m14.5 6-6 6 6 6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const chevronRight = `<svg viewBox="0 0 24 24" fill="none" width="14" height="14" aria-hidden="true"><path d="m9.5 6 6 6-6 6" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
</script>

<template>
  <div ref="rootEl" class="dtp" :class="{ open }" @keydown.escape.stop="close">
    <button
      type="button"
      class="af-input dtp-trigger"
      :class="{ 'dtp-placeholder': !display }"
      :aria-expanded="open"
      aria-haspopup="dialog"
      @click="toggle"
    >
      <span class="dtp-value mono">{{ display || placeholder || t("datePicker.placeholder") }}</span>
      <span v-if="parts" class="dtp-clear" role="button" :title="t('datePicker.clear')" :aria-label="t('datePicker.clear')" @click.stop="clear">✕</span>
      <span class="dtp-icon" v-html="calendarIcon" />
    </button>

    <div v-if="open" class="dtp-pop af-panel" :class="placement === 'up' ? 'dtp-up' : 'dtp-down'" role="dialog" :aria-label="t('datePicker.title')">
      <div class="dtp-head">
        <button type="button" class="dtp-nav" :title="t('datePicker.prevMonth')" :aria-label="t('datePicker.prevMonth')" @click="prevMonth" v-html="chevronLeft" />
        <span class="dtp-month">{{ monthLabel }}</span>
        <button type="button" class="dtp-nav" :title="t('datePicker.nextMonth')" :aria-label="t('datePicker.nextMonth')" @click="nextMonth" v-html="chevronRight" />
      </div>

      <div class="dtp-week">
        <span v-for="(w, i) in weekdayLabels" :key="i" class="dtp-weekday">{{ w }}</span>
      </div>

      <div ref="gridEl" class="dtp-grid" role="grid" @keydown="onGridKeydown">
        <button
          v-for="cell in cells"
          :key="cell.key"
          type="button"
          class="dtp-day"
          data-day
          :class="{ outside: !cell.inMonth, today: cell.isToday, selected: cell.isSelected }"
          :aria-pressed="cell.isSelected"
          @click="pickCell(cell)"
        >{{ cell.day }}</button>
      </div>

      <div class="dtp-time">
        <span class="dtp-time-label">{{ t("datePicker.time") }}</span>
        <input
          class="af-input mono dtp-time-input"
          type="number"
          min="0"
          max="23"
          :value="hourValue"
          :aria-label="t('datePicker.hour')"
          @change="setHour(($event.target as HTMLInputElement).value)"
        />
        <span class="dtp-colon">:</span>
        <input
          class="af-input mono dtp-time-input"
          type="number"
          min="0"
          max="59"
          :value="minuteValue"
          :aria-label="t('datePicker.minute')"
          @change="setMinute(($event.target as HTMLInputElement).value)"
        />
        <span class="dtp-time-spacer" />
        <button type="button" class="dtp-mini" @click="pickToday">{{ t("datePicker.today") }}</button>
      </div>

      <div class="dtp-foot">
        <button type="button" class="dtp-mini danger" @click="clear">{{ t("datePicker.clear") }}</button>
        <button type="button" class="dtp-mini primary" @click="close">{{ t("datePicker.done") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dtp {
  position: relative;
  display: inline-block;
  width: 100%;
}
.dtp-trigger {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  cursor: pointer;
  padding: 8px 10px;
  font-size: 13px;
}
.dtp-trigger:hover {
  border-color: var(--af-text-dim);
}
.dtp-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.dtp-placeholder .dtp-value {
  color: var(--af-text-dim);
}
.dtp-clear {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  font-size: 11px;
  color: var(--af-text-dim);
  cursor: pointer;
  user-select: none;
}
.dtp-clear:hover {
  background: var(--af-accent-soft);
  color: var(--af-accent);
}
.dtp-icon {
  display: inline-flex;
  color: var(--af-text-dim);
}
.dtp-pop {
  position: absolute;
  z-index: 60;
  left: 0;
  width: 252px;
  padding: 10px;
  display: flex;
  flex-direction: column;
  gap: 8px;
  box-shadow: var(--af-shadow);
}
.dtp-down {
  top: calc(100% + 6px);
}
.dtp-up {
  bottom: calc(100% + 6px);
}
.dtp-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
}
.dtp-month {
  font-size: 12.5px;
  font-weight: 650;
  color: var(--af-text);
}
.dtp-nav {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: 0;
  border-radius: var(--af-radius-sm);
  background: transparent;
  color: var(--af-text-dim);
  cursor: pointer;
}
.dtp-nav:hover {
  background: var(--af-accent-soft);
  color: var(--af-accent);
}
.dtp-week {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}
.dtp-weekday {
  text-align: center;
  font-size: 10.5px;
  color: var(--af-text-dim);
  padding: 2px 0;
  user-select: none;
}
.dtp-grid {
  display: grid;
  grid-template-columns: repeat(7, 1fr);
  gap: 2px;
}
.dtp-day {
  height: 30px;
  border: 1px solid transparent;
  border-radius: var(--af-radius-sm);
  background: transparent;
  color: var(--af-text);
  font-size: 12px;
  cursor: pointer;
}
.dtp-day:hover {
  background: var(--af-accent-soft);
}
.dtp-day:focus-visible {
  outline: 2px solid var(--af-accent);
  outline-offset: -1px;
}
.dtp-day.outside {
  color: var(--af-text-dim);
  opacity: 0.45;
}
.dtp-day.today {
  border-color: var(--af-accent);
}
.dtp-day.selected,
.dtp-day.selected:hover {
  background: var(--af-accent);
  border-color: var(--af-accent);
  color: #ffffff;
}
.dtp-time {
  display: flex;
  align-items: center;
  gap: 6px;
  border-top: 1px solid var(--af-border);
  padding-top: 8px;
}
.dtp-time-label {
  font-size: 12px;
  color: var(--af-text-dim);
}
.dtp-time-input {
  width: 48px;
  padding: 4px 6px;
  font-size: 12px;
  text-align: center;
}
.dtp-colon {
  color: var(--af-text-dim);
  font-weight: 700;
}
.dtp-time-spacer {
  flex: 1;
}
.dtp-foot {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.dtp-mini {
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text);
  border-radius: var(--af-radius-sm);
  padding: 5px 10px;
  font-size: 11.5px;
  cursor: pointer;
}
.dtp-mini:hover {
  border-color: var(--af-text-dim);
}
.dtp-mini.primary {
  background: var(--af-accent);
  border-color: var(--af-accent);
  color: #ffffff;
}
.dtp-mini.primary:hover {
  filter: brightness(1.05);
}
.dtp-mini.danger {
  color: var(--af-danger);
}
@keyframes dtp-in {
  from {
    opacity: 0;
    transform: translateY(-4px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.dtp-pop {
  animation: dtp-in 0.12s ease;
}
@media (prefers-reduced-motion: reduce) {
  .dtp-pop {
    animation: none;
  }
}
</style>
