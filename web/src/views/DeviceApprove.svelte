<script>
  import SignIn from './SignIn.svelte';
  import SignUp from './SignUp.svelte';
  import TotpSetup from './TotpSetup.svelte';
  import { api } from '../lib/api.js';
  import { t, errorMessage } from '../lib/i18n.svelte.js';

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
      error = errorMessage(e.code, e.message);
    } finally {
      approving = false;
    }
  }
</script>

{#if stage === 'signin'}
  <p class="subtitle">
    {t('device.signin_title')}{#if code} {t('device.with_code', { code })}{/if}.
  </p>
  {#if !userCode}
    <div class="field">
      <label for="code">{t('device.code_label')}</label>
      <input
        id="code"
        type="text"
        bind:value={enteredCode}
        placeholder={t('device.code_placeholder')}
        autocomplete="off"
      />
    </div>
  {/if}
  <SignIn {onAuthenticated} />
  <div class="footer">
    {t('common.no_account')} <button onclick={() => (stage = 'signup')}>{t('common.create_account')}</button>
  </div>
{:else if stage === 'signup'}
  <p class="subtitle">{t('device.signup_title')}</p>
  <SignUp {onRegistered} />
  <div class="footer">
    {t('common.already_have_account')}
    <button onclick={() => (stage = 'signin')}>{t('common.sign_in')}</button>
  </div>
{:else if stage === 'totp-setup'}
  <p class="subtitle">{t('device.totp_title')}</p>
  <TotpSetup {registration} {onTotpConfirmed} />
{:else if stage === 'approve'}
  {#if error}
    <div class="error">{error}</div>
  {/if}
  <p class="subtitle">{t('device.approve_title')}</p>
  <div class="secret">{code}</div>
  <div class="row">
    <button onclick={approve} disabled={approving || !code}>
      {approving ? t('device.authorizing') : t('device.authorize')}
    </button>
  </div>
{:else}
  <div class="notice">
    {t('device.approved')}
  </div>
{/if}
