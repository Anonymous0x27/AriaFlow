<script setup lang="ts">
/**
 * Shared destructive-action confirmation. Optionally shows a single checkbox
 * (e.g. "also delete the downloaded file") and reports it via `confirm`.
 */
import { computed, ref, watch } from "vue";
import { useI18n } from "../lib/i18n";

const props = defineProps<{
  open: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  checkboxLabel?: string;
  checkboxDefault?: boolean;
  danger?: boolean;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "confirm", deleteFile: boolean): void;
}>();

const { t } = useI18n();
const checkbox = ref(false);
const showCheckbox = computed(() => Boolean(props.checkboxLabel));

watch(
  () => props.open,
  (open) => {
    if (open) checkbox.value = props.checkboxDefault ?? false;
  },
);
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog">
    <div v-if="open" class="cd-overlay" @click.self="emit('close')">
      <div class="cd-panel" role="alertdialog" aria-modal="true" :aria-label="title" @keydown.escape.stop="emit('close')">
        <div class="cd-icon" :class="{ danger }" aria-hidden="true">
          <svg viewBox="0 0 24 24" fill="none" width="22" height="22">
            <path d="M12 3 2.6 20h18.8L12 3Z" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round"/>
            <path d="M12 9.5v4.6M12 16.9h.01" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
          </svg>
        </div>
        <div class="cd-body">
          <h3 class="cd-title">{{ title }}</h3>
          <p class="cd-message">{{ message }}</p>
          <label v-if="showCheckbox" class="cd-check">
            <input v-model="checkbox" type="checkbox" />
            <span>{{ checkboxLabel }}</span>
          </label>
        </div>
        <div class="cd-actions">
          <button class="af-btn" type="button" @click="emit('close')">{{ t("common.cancel") }}</button>
          <button class="af-btn" :class="{ 'af-btn-danger-solid': danger }" type="button" @click="emit('confirm', checkbox)">
            {{ confirmLabel ?? t("confirm.delete") }}
          </button>
        </div>
      </div>
    </div>
    </Transition>
  </Teleport>
</template>
