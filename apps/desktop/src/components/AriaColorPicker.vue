<script setup lang="ts">
/**
 * AriaFlow-themed accent color picker — replaces the native <input type="color">,
 * whose popover is drawn by the browser/OS and therefore ignores the app's theme
 * (and follows the browser's own UI colour scheme instead).
 *
 * Everything is plain Vue + CSS variables: SV gradient area + hue slider.
 * v-model contract: "#rrggbb" (lowercase, any case accepted for input).
 */
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { useI18n } from "../lib/i18n";

const props = withDefaults(
  defineProps<{
    modelValue: string;
    /** Ring highlight on the trigger (e.g. custom, non-preset accent is active). */
    active?: boolean;
    title?: string;
  }>(),
  { active: false, title: "" },
);

const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();

const { t } = useI18n();
const open = ref(false);
const rootEl = ref<HTMLElement | null>(null);
const triggerEl = ref<HTMLButtonElement | null>(null);
const svEl = ref<HTMLElement | null>(null);
const hueEl = ref<HTMLElement | null>(null);
/** Resolved direction: open upward when there isn't room below the trigger. */
const openUp = ref(false);

const POP_HEIGHT = 232; // sv 140 + hue 14 + foot ~24 + paddings/gaps + offset

// ---------- color math ----------
interface HSV { h: number; s: number; v: number } // h 0-360, s/v 0-100

function normalizeHex(input: string): string | null {
  const raw = String(input ?? "").trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{3}$/.test(raw)) {
    return ("#" + raw.split("").map((c) => c + c).join("")).toLowerCase();
  }
  if (/^[0-9a-fA-F]{6}$/.test(raw)) return "#" + raw.toLowerCase();
  return null;
}

function hexToRgb(hex: string): [number, number, number] | null {
  const n = normalizeHex(hex);
  if (!n) return null;
  return [parseInt(n.slice(1, 3), 16), parseInt(n.slice(3, 5), 16), parseInt(n.slice(5, 7), 16)];
}

function rgbToHex(r: number, g: number, b: number): string {
  const p = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
  return `#${p(r)}${p(g)}${p(b)}`;
}

function rgbToHsv(r: number, g: number, b: number): HSV {
  r /= 255; g /= 255; b /= 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  const d = max - min;
  let h = 0;
  if (d > 0) {
    if (max === r) h = ((g - b) / d) % 6;
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  const s = max === 0 ? 0 : d / max;
  return { h, s: s * 100, v: max * 100 };
}

function hsvToRgb(h: number, s: number, v: number): [number, number, number] {
  const S = s / 100, V = v / 100;
  const c = V * S;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = V - c;
  let rp = 0, gp = 0, bp = 0;
  if (h < 60) [rp, gp, bp] = [c, x, 0];
  else if (h < 120) [rp, gp, bp] = [x, c, 0];
  else if (h < 180) [rp, gp, bp] = [0, c, x];
  else if (h < 240) [rp, gp, bp] = [0, x, c];
  else if (h < 300) [rp, gp, bp] = [x, 0, c];
  else [rp, gp, bp] = [c, 0, x];
  return [(rp + m) * 255, (gp + m) * 255, (bp + m) * 255];
}

// ---------- state ----------
const initial = (() => {
  const rgb = hexToRgb(props.modelValue) ?? [10, 132, 255];
  return rgbToHsv(rgb[0], rgb[1], rgb[2]);
})();
const h = ref(initial.h);
const s = ref(initial.s);
const v = ref(initial.v);

const currentHex = computed(() => {
  const [r, g, b] = hsvToRgb(h.value, s.value, v.value);
  return rgbToHex(r, g, b);
});
const pureHue = computed(() => {
  const [r, g, b] = hsvToRgb(h.value, 100, 100);
  return `rgb(${Math.round(r)}, ${Math.round(g)}, ${Math.round(b)})`;
});

/** Re-sync from the outside world (preset swatch clicked, hex field committed, …). */
watch(
  () => props.modelValue,
  (value) => {
    const rgb = hexToRgb(value);
    if (!rgb) return;
    if (normalizeHex(value) === currentHex.value) return; // echo of our own emit
    const next = rgbToHsv(rgb[0], rgb[1], rgb[2]);
    h.value = next.h;
    // A grey/black target has no meaningful saturation/hue; keep the wheel where it is.
    if (next.s > 0) s.value = next.s;
    v.value = next.v;
  },
);

function emitColor() {
  emit("update:modelValue", currentHex.value);
}

// ---------- pointer interactions ----------
function svFromPointer(event: PointerEvent) {
  const el = svEl.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const x = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  const y = Math.min(1, Math.max(0, (event.clientY - rect.top) / rect.height));
  s.value = x * 100;
  v.value = (1 - y) * 100;
  emitColor();
}

function hueFromPointer(event: PointerEvent) {
  const el = hueEl.value;
  if (!el) return;
  const rect = el.getBoundingClientRect();
  const x = Math.min(1, Math.max(0, (event.clientX - rect.left) / rect.width));
  h.value = x * 360;
  emitColor();
}

function onSvPointerDown(event: PointerEvent) {
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  svFromPointer(event);
}
function onSvPointerMove(event: PointerEvent) {
  if ((event.currentTarget as HTMLElement).hasPointerCapture(event.pointerId)) svFromPointer(event);
}
function onHuePointerDown(event: PointerEvent) {
  (event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);
  hueFromPointer(event);
}
function onHuePointerMove(event: PointerEvent) {
  if ((event.currentTarget as HTMLElement).hasPointerCapture(event.pointerId)) hueFromPointer(event);
}

// ---------- keyboard ----------
function onSvKeydown(event: KeyboardEvent) {
  const step = event.shiftKey ? 10 : 2;
  if (event.key === "ArrowLeft") s.value = Math.max(0, s.value - step);
  else if (event.key === "ArrowRight") s.value = Math.min(100, s.value + step);
  else if (event.key === "ArrowUp") v.value = Math.min(100, v.value + step);
  else if (event.key === "ArrowDown") v.value = Math.max(0, v.value - step);
  else return;
  event.preventDefault();
  emitColor();
}
function onHueKeydown(event: KeyboardEvent) {
  const step = event.shiftKey ? 12 : 2;
  if (event.key === "ArrowLeft" || event.key === "ArrowDown") h.value = (h.value - step + 360) % 360;
  else if (event.key === "ArrowRight" || event.key === "ArrowUp") h.value = (h.value + step) % 360;
  else return;
  event.preventDefault();
  emitColor();
}

function toggle() {
  open.value = !open.value;
  if (open.value && triggerEl.value && typeof window !== "undefined") {
    const rect = triggerEl.value.getBoundingClientRect();
    openUp.value = rect.bottom + POP_HEIGHT + 8 > window.innerHeight && rect.top > POP_HEIGHT + 8;
  }
}
function close() {
  open.value = false;
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

const svStyle = computed(() => ({ background: pureHue.value }));
const cursorStyle = computed(() => ({ left: `${s.value}%`, top: `${100 - v.value}%` }));
const hueThumbStyle = computed(() => ({ left: `${(h.value / 360) * 100}%` }));
const swatchStyle = computed(() => ({ background: currentHex.value }));
</script>

<template>
  <div ref="rootEl" class="acp" :class="{ open }" @keydown.escape.stop="close">
    <button
      ref="triggerEl"
      type="button"
      class="acp-trigger"
      :class="{ active: active || open }"
      :title="title || t('colorPicker.title')"
      :aria-label="title || t('colorPicker.title')"
      aria-haspopup="dialog"
      :aria-expanded="open"
      @click="toggle"
    />

    <div v-if="open" class="acp-pop af-panel" :class="{ 'acp-up': openUp }" role="dialog" :aria-label="t('colorPicker.title')">
      <div
        ref="svEl"
        class="acp-sv"
        :style="svStyle"
        role="slider"
        tabindex="0"
        :aria-label="t('colorPicker.saturationValue')"
        :aria-valuetext="`S ${Math.round(s)}%, V ${Math.round(v)}%`"
        :aria-valuenow="Math.round(s)"
        aria-valuemin="0"
        aria-valuemax="100"
        @pointerdown="onSvPointerDown"
        @pointermove="onSvPointerMove"
        @keydown="onSvKeydown"
      >
        <div class="acp-sv-white" />
        <div class="acp-sv-black" />
        <div class="acp-sv-cursor" :style="cursorStyle" />
      </div>

      <div
        ref="hueEl"
        class="acp-hue"
        role="slider"
        tabindex="0"
        :aria-label="t('colorPicker.hue')"
        :aria-valuenow="Math.round(h)"
        aria-valuemin="0"
        aria-valuemax="360"
        @pointerdown="onHuePointerDown"
        @pointermove="onHuePointerMove"
        @keydown="onHueKeydown"
      >
        <div class="acp-hue-thumb" :style="hueThumbStyle" />
      </div>

      <div class="acp-foot">
        <span class="acp-swatch" :style="swatchStyle" />
        <span class="acp-hex mono">{{ currentHex }}</span>
        <span class="acp-spacer" />
        <button type="button" class="acp-done" @click="close">{{ t("datePicker.done") }}</button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.acp {
  position: relative;
  display: inline-flex;
}
.acp-trigger {
  width: 26px;
  height: 26px;
  border-radius: 50%;
  border: 2px solid transparent;
  outline: 2px solid transparent;
  outline-offset: 2px;
  cursor: pointer;
  background: conic-gradient(#ff375f, #ff9f0a, #34c759, #0a84ff, #bf5af2, #ff375f);
  padding: 0;
}
.acp-trigger.active {
  outline-color: var(--af-accent);
}
.acp-trigger:hover,
.acp-trigger.active {
  background: conic-gradient(#ff375f, #ff9f0a, #34c759, #0a84ff, #bf5af2, #ff375f);
}
.acp-pop {
  position: absolute;
  z-index: 60;
  top: calc(100% + 8px);
  left: 0;
  width: 216px;
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 10px;
  box-shadow: var(--af-shadow);
  animation: acp-in 0.12s ease;
}
@keyframes acp-in {
  from {
    opacity: 0;
    transform: translateY(-4px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.acp-up {
  top: auto;
  bottom: calc(100% + 8px);
}
@keyframes acp-up-in {
  from {
    opacity: 0;
    transform: translateY(4px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.acp-up {
  animation-name: acp-up-in;
}
.acp-sv {
  position: relative;
  height: 140px;
  border-radius: var(--af-radius-sm);
  overflow: hidden;
  cursor: crosshair;
  touch-action: none;
  border: 1px solid var(--af-border);
}
.acp-sv:focus-visible {
  outline: 2px solid var(--af-accent);
  outline-offset: 1px;
}
.acp-sv-white {
  position: absolute;
  inset: 0;
  background: linear-gradient(to right, #ffffff, rgba(255, 255, 255, 0));
}
.acp-sv-black {
  position: absolute;
  inset: 0;
  background: linear-gradient(to top, #000000, rgba(0, 0, 0, 0));
}
.acp-sv-cursor {
  position: absolute;
  width: 13px;
  height: 13px;
  margin: -6.5px 0 0 -6.5px;
  border-radius: 50%;
  border: 2px solid #ffffff;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.45), 0 1px 3px rgba(0, 0, 0, 0.5);
  pointer-events: none;
}
.acp-hue {
  position: relative;
  height: 14px;
  border-radius: 7px;
  cursor: pointer;
  touch-action: none;
  background: linear-gradient(
    to right,
    #ff0000,
    #ffff00,
    #00ff00,
    #00ffff,
    #0000ff,
    #ff00ff,
    #ff0000
  );
  border: 1px solid var(--af-border);
}
.acp-hue:focus-visible {
  outline: 2px solid var(--af-accent);
  outline-offset: 1px;
}
.acp-hue-thumb {
  position: absolute;
  top: 50%;
  width: 16px;
  height: 16px;
  margin-left: -8px;
  transform: translateY(-50%);
  border-radius: 50%;
  background: #ffffff;
  border: 1px solid var(--af-border);
  box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
  pointer-events: none;
}
.acp-foot {
  display: flex;
  align-items: center;
  gap: 8px;
}
.acp-swatch {
  width: 22px;
  height: 22px;
  border-radius: var(--af-radius-sm);
  border: 1px solid var(--af-border);
  flex-shrink: 0;
}
.acp-hex {
  font-size: 12px;
  color: var(--af-text);
  letter-spacing: 0.02em;
}
.acp-spacer {
  flex: 1;
}
.acp-done {
  border: 1px solid var(--af-border);
  background: var(--af-panel-solid);
  color: var(--af-text);
  border-radius: var(--af-radius-sm);
  padding: 5px 12px;
  font-size: 11.5px;
  cursor: pointer;
}
.acp-done:hover {
  border-color: var(--af-text-dim);
}
@media (prefers-reduced-motion: reduce) {
  .acp-pop {
    animation: none;
  }
}
</style>
