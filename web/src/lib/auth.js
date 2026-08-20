import { writable } from 'svelte/store';

const KEY = 'light_factory_session';

function load() {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

export const session = writable(load());

export function setSession(value) {
  session.set(value);
  if (value) {
    localStorage.setItem(KEY, JSON.stringify(value));
  } else {
    localStorage.removeItem(KEY);
  }
}
