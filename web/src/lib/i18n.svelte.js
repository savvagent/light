import {
  DEFAULT_LOCALE,
  STORAGE_KEY,
  normalizeLocale,
  resolveLocale,
  translate,
  translateErrorCode,
} from './i18n.js';

function readSaved() {
  try {
    return localStorage.getItem(STORAGE_KEY);
  } catch {
    return null;
  }
}

function persist(locale) {
  try {
    localStorage.setItem(STORAGE_KEY, locale);
  } catch {
    // storage may be unavailable (private mode); the preference simply won't persist
  }
}

const initial = resolveLocale({
  saved: typeof localStorage !== 'undefined' ? readSaved() : null,
  navigatorLanguage: typeof navigator !== 'undefined' ? navigator.language : null,
});

export const locale = $state({ value: initial });

export function setLocale(raw) {
  const next = normalizeLocale(raw) ?? DEFAULT_LOCALE;
  locale.value = next;
  persist(next);
}

export function t(key, params) {
  return translate(locale.value, key, params);
}

export function errorMessage(code, fallback) {
  return translateErrorCode(locale.value, code) ?? fallback;
}
