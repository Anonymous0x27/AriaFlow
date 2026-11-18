/**
 * Shared, app-wide notice (toast) for transient, non-blocking messages —
 * e.g. daemon rejecting a download URL. Rendered once by `App.vue` in the
 * toolbar; every view/dialog can raise it without needing a prop or emit.
 *
 * Usage:
 *   import { showNotice } from "../lib/notice";
 *   showNotice(t("some.message"));
 */
import { reactive } from "vue";

export const noticeState = reactive<{ message: string | null }>({ message: null });

let noticeTimer: number | undefined;

export function showNotice(message: string, durationMs = 5000): void {
  noticeState.message = message;
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => {
    noticeState.message = null;
  }, durationMs);
}

export function clearNotice(): void {
  window.clearTimeout(noticeTimer);
  noticeState.message = null;
}
