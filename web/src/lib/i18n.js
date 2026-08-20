// Translation catalog and locale resolution for the web client.
//
// This module is intentionally free of Svelte imports so it can be unit-tested
// directly with node. The reactive store lives in `i18n.svelte.js`.

export const DEFAULT_LOCALE = 'en';
export const SUPPORTED_LOCALES = ['en', 'es'];
export const STORAGE_KEY = 'light_factory_locale';

export const catalogs = {
  en: {
    'common.email': 'Email',
    'common.display_name': 'Display name',
    'common.sign_in': 'Sign in',
    'common.sign_out': 'Sign out',
    'common.create_account': 'Create account',
    'common.no_account': 'No account?',
    'common.already_have_account': 'Already have an account?',
    'common.language': 'Language',

    'signin.title': 'Sign in with your authenticator app.',
    'signin.code_label': 'Authenticator code',
    'signin.submitting': 'Signing in…',
    'signin.submit': 'Sign in',

    'signup.title': 'Create your account.',
    'signup.name_label': 'Display name (optional)',
    'signup.submitting': 'Creating account…',
    'signup.submit': 'Create account',

    'totp.title': 'Secure your account with an authenticator app.',
    'totp.generating': 'Generating your authenticator setup…',
    'totp.scan':
      'Scan this QR code with your authenticator app (Google Authenticator, 1Password, etc.), then enter the code it shows.',
    'totp.qr_alt': 'TOTP QR code',
    'totp.manual': 'Or enter this secret manually:',
    'totp.code_label': 'Confirmation code',
    'totp.verifying': 'Verifying…',
    'totp.confirm': 'Confirm and continue',

    'dashboard.subtitle': "You're logged in — create something great.",
    'dashboard.email': 'Email',
    'dashboard.display_name': 'Display name',
    'dashboard.signin_method': 'Sign-in method',
    'dashboard.method_authenticator': 'Authenticator app',
    'dashboard.sign_out': 'Sign out',

    'device.signin_title': 'Sign in to authorize your terminal',
    'device.with_code': 'with code {code}',
    'device.code_label': 'Device code',
    'device.code_placeholder': 'ABCD-EFGH',
    'device.signup_title': 'Create an account to authorize your terminal.',
    'device.totp_title': 'Secure your account, then authorize your terminal.',
    'device.approve_title': 'A terminal on your machine is requesting access.',
    'device.authorizing': 'Authorizing…',
    'device.authorize': 'Authorize',
    'device.approved':
      'Authorized. You can close this window — your terminal is now signed in.',

    'error.invalid_credentials': 'Invalid email or code.',
    'error.email_taken': 'An account with that email already exists.',
    'error.invalid_email': 'Invalid email address.',
    'error.invalid_totp_code': 'Invalid TOTP code.',
    'error.invalid_challenge': 'Invalid or expired challenge.',
    'error.invalid_session': 'Invalid or expired session.',
    'error.invalid_grant': 'Invalid device code.',
    'error.expired_token': 'Device authorization expired.',
    'error.storage_error': 'Storage error.',
    'error.internal_error': 'Internal server error.',
    'error.invalid_json': 'Invalid request.',
    'error.network': 'Could not reach the server.',
    'error.decode': 'Unexpected response from the server.',
  },
  es: {
    'common.email': 'Correo electrónico',
    'common.display_name': 'Nombre visible',
    'common.sign_in': 'Iniciar sesión',
    'common.sign_out': 'Cerrar sesión',
    'common.create_account': 'Crear cuenta',
    'common.no_account': '¿No tienes cuenta?',
    'common.already_have_account': '¿Ya tienes una cuenta?',
    'common.language': 'Idioma',

    'signin.title': 'Inicia sesión con tu aplicación de autenticación.',
    'signin.code_label': 'Código de autenticación',
    'signin.submitting': 'Iniciando sesión…',
    'signin.submit': 'Iniciar sesión',

    'signup.title': 'Crea tu cuenta.',
    'signup.name_label': 'Nombre visible (opcional)',
    'signup.submitting': 'Creando cuenta…',
    'signup.submit': 'Crear cuenta',

    'totp.title': 'Protege tu cuenta con una aplicación de autenticación.',
    'totp.generating': 'Generando la configuración de tu autenticador…',
    'totp.scan':
      'Escanea este código QR con tu aplicación de autenticación (Google Authenticator, 1Password, etc.) y luego introduce el código que muestra.',
    'totp.qr_alt': 'Código QR TOTP',
    'totp.manual': 'O introduce este secreto manualmente:',
    'totp.code_label': 'Código de confirmación',
    'totp.verifying': 'Verificando…',
    'totp.confirm': 'Confirmar y continuar',

    'dashboard.subtitle': 'Has iniciado sesión: crea algo genial.',
    'dashboard.email': 'Correo electrónico',
    'dashboard.display_name': 'Nombre visible',
    'dashboard.signin_method': 'Método de inicio de sesión',
    'dashboard.method_authenticator': 'Aplicación de autenticación',
    'dashboard.sign_out': 'Cerrar sesión',

    'device.signin_title': 'Inicia sesión para autorizar tu terminal',
    'device.with_code': 'con el código {code}',
    'device.code_label': 'Código del dispositivo',
    'device.code_placeholder': 'ABCD-EFGH',
    'device.signup_title': 'Crea una cuenta para autorizar tu terminal.',
    'device.totp_title': 'Protege tu cuenta y luego autoriza tu terminal.',
    'device.approve_title': 'Un terminal en tu equipo está solicitando acceso.',
    'device.authorizing': 'Autorizando…',
    'device.authorize': 'Autorizar',
    'device.approved':
      'Autorizado. Ya puedes cerrar esta ventana: tu terminal ha iniciado sesión.',

    'error.invalid_credentials': 'Correo o código no válidos.',
    'error.email_taken': 'Ya existe una cuenta con ese correo electrónico.',
    'error.invalid_email': 'Dirección de correo electrónico no válida.',
    'error.invalid_totp_code': 'Código TOTP no válido.',
    'error.invalid_challenge': 'Desafío no válido o caducado.',
    'error.invalid_session': 'Sesión no válida o caducada.',
    'error.invalid_grant': 'Código de dispositivo no válido.',
    'error.expired_token': 'La autorización del dispositivo ha caducado.',
    'error.storage_error': 'Error de almacenamiento.',
    'error.internal_error': 'Error interno del servidor.',
    'error.invalid_json': 'Solicitud no válida.',
    'error.network': 'No se pudo contactar con el servidor.',
    'error.decode': 'Respuesta inesperada del servidor.',
  },
};

export function normalizeLocale(raw) {
  if (!raw) return null;
  const base = String(raw).toLowerCase().split(/[-_]/)[0];
  return SUPPORTED_LOCALES.includes(base) ? base : null;
}

export function resolveLocale({ saved, navigatorLanguage } = {}) {
  const fromSaved = normalizeLocale(saved);
  if (fromSaved) return fromSaved;
  const fromNavigator = normalizeLocale(navigatorLanguage);
  if (fromNavigator) return fromNavigator;
  return DEFAULT_LOCALE;
}

export function translate(locale, key, params) {
  const catalog = catalogs[locale] ?? catalogs[DEFAULT_LOCALE];
  let text = catalog[key] ?? catalogs[DEFAULT_LOCALE][key] ?? key;
  if (params) {
    for (const [name, value] of Object.entries(params)) {
      text = text.split(`{${name}}`).join(String(value));
    }
  }
  return text;
}

// Map a stable server/client error code to a localized message, or null when the
// code is unrecognized (callers then surface the server message verbatim).
export function translateErrorCode(locale, code) {
  if (!code) return null;
  const key = `error.${code}`;
  const catalog = catalogs[locale] ?? catalogs[DEFAULT_LOCALE];
  return catalog[key] ?? null;
}
