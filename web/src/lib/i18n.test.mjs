import assert from 'node:assert/strict';
import {
  DEFAULT_LOCALE,
  SUPPORTED_LOCALES,
  STORAGE_KEY,
  catalogs,
  normalizeLocale,
  resolveLocale,
  translate,
  translateErrorCode,
} from './i18n.js';

// (a) es is a complete mirror of en: same keys, no extras.
assert.deepEqual(
  Object.keys(catalogs.es).sort(),
  Object.keys(catalogs.en).sort(),
  'es catalog must mirror en keys exactly',
);

assert.equal(SUPPORTED_LOCALES.includes('en'), true);
assert.equal(SUPPORTED_LOCALES.includes('es'), true);
assert.equal(DEFAULT_LOCALE, 'en');
assert.equal(STORAGE_KEY, 'light_factory_locale');

// (b) normalization.
assert.equal(normalizeLocale('es-MX'), 'es');
assert.equal(normalizeLocale('EN-US'), 'en');
assert.equal(normalizeLocale('en_US'), 'en');
assert.equal(normalizeLocale('fr'), null);
assert.equal(normalizeLocale(null), null);

// (c) resolution precedence: saved > navigator > default.
assert.equal(resolveLocale({ saved: 'es', navigatorLanguage: 'en' }), 'es');
assert.equal(resolveLocale({ saved: null, navigatorLanguage: 'es-ES' }), 'es');
assert.equal(resolveLocale({ saved: 'fr', navigatorLanguage: 'de' }), 'en');
assert.equal(resolveLocale({}), 'en');

// (d) es actually translates.
assert.notEqual(translate('es', 'signin.title'), translate('en', 'signin.title'));
assert.notEqual(
  translate('es', 'error.invalid_credentials'),
  translate('en', 'error.invalid_credentials'),
);

// (e) fallback: a key always resolves to English when the target locale lacks it; never throws.
assert.equal(translate('en', 'totp.qr_alt'), 'TOTP QR code');
assert.equal(translate('es', 'dashboard.email'), 'Correo electrónico');

// (f) interpolation.
assert.equal(
  translate('en', 'device.with_code', { code: 'AB12' }),
  'with code AB12',
);
assert.equal(
  translate('es', 'device.with_code', { code: 'AB12' }),
  'con el código AB12',
);

// (g) error-code translation + verbatim fallback contract.
assert.equal(translateErrorCode('es', 'invalid_credentials'), 'Correo o código no válidos.');
assert.equal(translateErrorCode('es', 'unknown'), null);
assert.equal(translateErrorCode('en', 'network'), 'Could not reach the server.');

// (h) unknown key falls back to English, and to the key only when English lacks it.
assert.equal(translate('es', 'signin.submit'), 'Iniciar sesión');
assert.equal(translate('es', 'definitely.not.a.key'), 'definitely.not.a.key');

console.log('i18n tests passed');
