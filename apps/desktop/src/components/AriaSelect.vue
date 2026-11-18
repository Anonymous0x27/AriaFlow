<script setup lang="ts">
/**
 * AriaFlow-themed select — replaces native <select>, whose option popup is drawn by the
 * browser/OS and therefore ignores the app's theme (light/dark/auto + custom accent).
 * Everything here is plain Vue + CSS variables, the same treatment as AriaDateTimePicker.
 *
 * v-model contract: the `value` of the selected option (strings only).
 */
import { computed, nextTick, onBeforeUnmount, ref, watch } from "vue";

export interface SelectOption {
  value: string;
  label: string;
}

const props = withDefaults(
  defineProps<{
    modelValue: string;
    options: SelectOption[];
    /** Shown (dimmed) when nothing matches, e.g. modelValue is not among options. */
    placeholder?: string;
    /** Which way the popover opens. "auto" (default) opens up when there is no room below. */
    placement?: "up" | "down" | "auto";
    /** Max width of the popover; the trigger stays 100% of its field. */
    popMaxWidth?: string;
  }>(),
  { placement: "auto", placeholder: "", popMaxWidth: "100%" },
);

const emit = defineEmits<{ (e: "update:modelValue", value: string): void }>();

const open = ref(false);
const rootEl = ref<HTMLElement | null>(null);
const triggerEl = ref<HTMLButtonElement | null>(null);
const listEl = ref<HTMLElement | null>(null);
const activeIndex = ref(-1);
/** Resolved direction, honouring placement="auto" flip logic. */
const openUp = ref(false);

function resolvePlacement() {
  if (props.placement === "up") return true;
  if (props.placement === "down") return false;
  const rect = triggerEl.value?.getBoundingClientRect();
  if (!rect || typeof window === "undefined") return false;
  const estimated = Math.min(props.options.length * 38 + 16, 236) + 8; // option rows + padding + offset
  return rect.bottom + estimated > window.innerHeight && rect.top > estimated;
}

const selectedIndex = computed(() => props.options.findIndex((o) => o.value === props.modelValue));
const selectedLabel = computed(() =>
  selectedIndex.value >= 0 ? props.options[selectedIndex.value].label : "",
);
const isPlaceholder = computed(() => selectedIndex.value < 0);

function scrollActiveIntoView() {
  void nextTick(() => {
    const list = listEl.value;
    if (!list) return;
    list.querySelector<HTMLElement>(".asel-option.active")?.scrollIntoView({ block: "nearest" });
  });
}

function toggle() {
  open.value = !open.value;
  if (open.value) {
    openUp.value = resolvePlacement();
    activeIndex.value = selectedIndex.value >= 0 ? selectedIndex.value : 0;
    scrollActiveIntoView();
  }
}

function close() {
  open.value = false;
}

function select(option: SelectOption) {
  emit("update:modelValue", option.value);
  open.value = false;
  void nextTick(() => triggerEl.value?.focus());
}

function moveActive(delta: number) {
  const n = props.options.length;
  if (n === 0) return;
  const base = activeIndex.value >= 0 ? activeIndex.value : selectedIndex.value;
  const start = base >= 0 ? base : 0;
  activeIndex.value = (start + delta + n) % n;
  scrollActiveIntoView();
}

function onKeydown(event: KeyboardEvent) {
  if (!open.value) {
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      toggle();
    }
    return;
  }
  switch (event.key) {
    case "ArrowDown":
      event.preventDefault();
      moveActive(1);
      break;
    case "ArrowUp":
      event.preventDefault();
      moveActive(-1);
      break;
    case "Home":
      event.preventDefault();
      activeIndex.value = 0;
      scrollActiveIntoView();
      break;
    case "End":
      event.preventDefault();
      activeIndex.value = props.options.length - 1;
      scrollActiveIntoView();
      break;
    case "Enter":
    case " ": {
      event.preventDefault();
      const target = props.options[activeIndex.value];
      if (target) select(target);
      break;
    }
    case "Escape":
      event.preventDefault();
      event.stopPropagation();
      close();
      void nextTick(() => triggerEl.value?.focus());
      break;
    case "Tab":
      close();
      break;
  }
}

watch(open, (value) => {
  if (value) activeIndex.value = selectedIndex.value >= 0 ? selectedIndex.value : 0;
});

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

const chevron = `<svg viewBox="0 0 24 24" fill="none" width="14" height="14" aria-hidden="true"><path d="m6 9 6 6 6-6" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
const checkIcon = `<svg viewBox="0 0 24 24" fill="none" width="13" height="13" aria-hidden="true"><path d="m5 12.5 4.5 4.5L19 7.5" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"/></svg>`;
</script>

<template>
  <div ref="rootEl" class="asel" :class="{ open }" @keydown="onKeydown">
    <button
      ref="triggerEl"
      type="button"
      class="af-input asel-trigger"
      :class="{ 'asel-placeholder': isPlaceholder }"
      aria-haspopup="listbox"
      :aria-expanded="open"
      @click="toggle"
    >
      <span class="asel-value" :title="selectedLabel">{{
        isPlaceholder ? placeholder : selectedLabel
      }}</span>
      <span class="asel-chevron" v-html="chevron" />
    </button>

    <div
      v-if="open"
      ref="listEl"
      class="asel-pop af-panel"
      :class="openUp ? 'asel-up' : 'asel-down'"
      :style="{ maxWidth: popMaxWidth }"
      role="listbox"
    >
      <button
        v-for="(option, index) in options"
        :key="option.value + ':' + index"
        type="button"
        class="asel-option"
        role="option"
        :class="{ selected: option.value === modelValue, active: index === activeIndex }"
        :aria-selected="option.value === modelValue"
        :title="option.label"
        @mouseenter="activeIndex = index"
        @click="select(option)"
      >
        <span class="asel-option-label">{{ option.label }}</span>
        <span v-if="option.value === modelValue" class="asel-check" v-html="checkIcon" />
      </button>
    </div>
  </div>
</template>

<style scoped>
.asel {
  position: relative;
  display: block;
  width: 100%;
  min-width: 0;
  max-width: 100%;
}
.asel-trigger {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  text-align: left;
  cursor: pointer;
  padding: 8px 10px;
  padding-right: 12px;
  font-size: 13px;
}
.asel-trigger:hover {
  border-color: var(--af-text-dim);
}
.asel-value {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.asel-placeholder .asel-value {
  color: var(--af-text-dim);
}
.asel-chevron {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--af-text-dim);
  transition: transform 0.15s ease;
}
.open .asel-chevron {
  transform: rotate(180deg);
}
.asel-pop {
  position: absolute;
  z-index: 60;
  left: 0;
  width: 100%;
  max-height: 236px;
  overflow-y: auto;
  padding: 4px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  box-shadow: var(--af-shadow);
}
.asel-down {
  top: calc(100% + 6px);
}
.asel-up {
  bottom: calc(100% + 6px);
}
.asel-option {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  border: 0;
  border-radius: var(--af-radius-sm);
  background: transparent;
  color: var(--af-text);
  font-size: 13px;
  text-align: left;
  padding: 7px 9px;
  cursor: pointer;
}
.asel-option-label {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.asel-option.active {
  background: var(--af-accent-soft);
}
.asel-option.selected {
  color: var(--af-accent);
  font-weight: 600;
}
.asel-option.selected.active {
  background: var(--af-accent-soft);
}
.asel-check {
  display: inline-flex;
  flex-shrink: 0;
  color: var(--af-accent);
}
@keyframes asel-in {
  from {
    opacity: 0;
    transform: translateY(-4px) scale(0.98);
  }
  to {
    opacity: 1;
    transform: none;
  }
}
.asel-pop {
  animation: asel-in 0.12s ease;
}
@media (prefers-reduced-motion: reduce) {
  .asel-pop {
    animation: none;
  }
}
</style>
