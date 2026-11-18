/**
 * Minimal i18n for the desktop UI — no runtime dependency.
 *
 * `useI18n()` exposes a reactive `locale` ref and `t(key, params?)`.
 * The active locale is persisted in localStorage (`ariaflow.locale`) and applied
 * to <html lang>. Lookup order: active locale → zh-CN (reference) → the key itself,
 * so a missing en string never renders blank.
 */
import { reactive } from "vue";
import { messages, type Locale } from "./messages";

const LOCALE_KEY = "ariaflow.locale";

function detectInitialLocale(): Locale {
  try {
    const saved = localStorage.getItem(LOCALE_KEY);
    if (saved === "zh-CN" || saved === "en") return saved;
  } catch {
    // ignore corrupted storage
  }
  const nav = typeof navigator !== "undefined" ? navigator.language : "zh-CN";
  return nav.toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

const state = reactive({ locale: detectInitialLocale() as Locale });

function interpolate(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (_, name: string) =>
    Object.prototype.hasOwnProperty.call(params, name) ? String(params[name]) : `{${name}}`,
  );
}

export function t(key: string, params?: Record<string, string | number>): string {
  const table = messages[state.locale] ?? {};
  const fallback = messages["zh-CN"] ?? {};
  const template = table[key] ?? fallback[key] ?? key;
  return interpolate(template, params);
}

export function setLocale(locale: Locale) {
  state.locale = locale;
  try {
    localStorage.setItem(LOCALE_KEY, locale);
  } catch {
    // non-fatal
  }
  if (typeof document !== "undefined") {
    document.documentElement.lang = locale;
  }
}

export function useI18n() {
  return { locale: state, t, setLocale };
}

/** Translated label for a download status; falls back to the raw status string. */
export function statusLabel(status: string): string {
  const key = `status.${status}`;
  const translated = t(key);
  return translated === key ? status : translated;
}

/** Translated label for a category key; falls back to the raw key. */
export function categoryLabel(category: string): string {
  const key = `category.${category}`;
  const translated = t(key);
  return translated === key ? category : translated;
}
