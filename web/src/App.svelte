<script>
  import { onMount } from 'svelte';
  import SignIn from './views/SignIn.svelte';
  import SignUp from './views/SignUp.svelte';
  import TotpSetup from './views/TotpSetup.svelte';
  import Dashboard from './views/Dashboard.svelte';
  import DeviceApprove from './views/DeviceApprove.svelte';
  import { api } from './lib/api.js';
  import { setSession } from './lib/auth.js';

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

  {#if view === 'device'}
    <DeviceApprove userCode={deviceCode} />
  {:else if view === 'signin'}
    <p class="subtitle">Sign in with your authenticator app.</p>
    <SignIn {onAuthenticated} />
    <div class="footer">
      No account? <button onclick={() => (view = 'signup')}>Create one</button>
    </div>
  {:else if view === 'signup'}
    <p class="subtitle">Create your account.</p>
    <SignUp {onRegistered} />
    <div class="footer">
      Already have an account?
      <button onclick={() => (view = 'signin')}>Sign in</button>
    </div>
  {:else if view === 'totp-setup'}
    <p class="subtitle">Secure your account with an authenticator app.</p>
    <TotpSetup {registration} {onTotpConfirmed} />
  {:else}
    <Dashboard user={auth.user} {onLogout} />
  {/if}
</main>
