<script>
  import { onMount } from 'svelte';
  import SignIn from './views/SignIn.svelte';
  import SignUp from './views/SignUp.svelte';
  import TotpSetup from './views/TotpSetup.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import DeviceApprove from './views/DeviceApprove.svelte';
  import { api } from './lib/api.js';
  import { setSession } from './lib/auth.js';
  import { t, locale, setLocale } from './lib/i18n.svelte.js';

  let view = $state('signin');
  let auth = $state(null);
  let registration = $state(null);
  let deviceCode = $state(null);

  onMount(() => {
    if (location.hash.startsWith('#/device')) {
      const q = location.hash.split('?')[1] ?? '';
      deviceCode = new URLSearchParams(q).get('user_code') ?? null;
      view = 'device';
    }
  });

  function applyAuth({ token, user }) {
    auth = { token, user };
    setSession({ token, user });
  }

  function onRegistered(resp) {
    registration = resp;
    view = 'totp-setup';
  }

  function onAuthenticated(resp) {
    applyAuth(resp);
    view = 'dashboard';
  }

  function onTotpConfirmed(resp) {
    applyAuth(resp);
    view = 'dashboard';
  }

  async function onLogout() {
    try {
      await api.logout(auth.token);
    } catch {
      // ignore: the session may already be invalid
    }
    auth = null;
    setSession(null);
    view = 'signin';
  }
</script>

<main class="card">
  <h1 class="brand">light<span class="dot">-factory</span></h1>

  <div class="locale">
    <label for="locale">{t('common.language')}</label>
    <select id="locale" value={locale.value} onchange={(e) => setLocale(e.currentTarget.value)}>
      <option value="en">English</option>
      <option value="es">Español</option>
    </select>
  </div>

  {#if view === 'device'}
    <DeviceApprove userCode={deviceCode} />
  {:else if view === 'signin'}
    <p class="subtitle">{t('signin.title')}</p>
    <SignIn {onAuthenticated} />
    <div class="footer">
      {t('common.no_account')} <button onclick={() => (view = 'signup')}>{t('common.create_account')}</button>
    </div>
  {:else if view === 'signup'}
    <p class="subtitle">{t('signup.title')}</p>
    <SignUp {onRegistered} />
    <div class="footer">
      {t('common.already_have_account')}
      <button onclick={() => (view = 'signin')}>{t('common.sign_in')}</button>
    </div>
  {:else if view === 'totp-setup'}
    <p class="subtitle">{t('totp.title')}</p>
    <TotpSetup {registration} {onTotpConfirmed} />
  {:else}
    <Dashboard user={auth.user} {onLogout} />
  {/if}
</main>
