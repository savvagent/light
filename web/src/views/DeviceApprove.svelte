<script>
  import SignIn from './SignIn.svelte';
  import SignUp from './SignUp.svelte';
  import TotpSetup from './TotpSetup.svelte';
  import { api } from '../lib/api.js';

  let { userCode } = $props();

  // signin | signup | totp-setup | approve | approved
  let stage = $state('signin');
  let token = $state(null);
  let registration = $state(null);
  let enteredCode = $state('');
  let approving = $state(false);
  let error = $state('');

  const code = $derived(userCode ?? enteredCode);

  function onAuthenticated(resp) {
    token = resp.token;
    stage = 'approve';
  }

  function onRegistered(resp) {
    registration = resp;
    stage = 'totp-setup';
  }

  function onTotpConfirmed(resp) {
    token = resp.token;
    stage = 'approve';
  }

  async function approve() {
    error = '';
    approving = true;
    try {
      await api.deviceApprove({ user_code: code }, token);
      stage = 'approved';
    } catch (e) {
      error = e.message;
    } finally {
      approving = false;
    }
  }
</script>

{#if stage === 'signin'}
  <p class="subtitle">
    Sign in to authorize your terminal{#if code} with code{' '}
      <strong>{code}</strong>{/if}.
  </p>
  {#if !userCode}
    <div class="field">
      <label for="code">Device code</label>
      <input
        id="code"
        type="text"
        bind:value={enteredCode}
        placeholder="ABCD-EFGH"
        autocomplete="off"
      />
    </div>
  {/if}
  <SignIn {onAuthenticated} />
  <div class="footer">
    No account? <button onclick={() => (stage = 'signup')}>Create one</button>
  </div>
{:else if stage === 'signup'}
  <p class="subtitle">Create an account to authorize your terminal.</p>
  <SignUp {onRegistered} />
  <div class="footer">
    Already have an account?
    <button onclick={() => (stage = 'signin')}>Sign in</button>
  </div>
{:else if stage === 'totp-setup'}
  <p class="subtitle">Secure your account, then authorize your terminal.</p>
  <TotpSetup {registration} {onTotpConfirmed} />
{:else if stage === 'approve'}
  {#if error}
    <div class="error">{error}</div>
  {/if}
  <p class="subtitle">A terminal on your machine is requesting access.</p>
  <div class="secret">{code}</div>
  <div class="row">
    <button onclick={approve} disabled={approving || !code}>
      {approving ? 'Authorizing…' : 'Authorize'}
    </button>
  </div>
{:else}
  <div class="notice">
    Authorized. You can close this window — your terminal is now signed in.
  </div>
{/if}
