<script>
  import { api } from '../lib/api.js';
  import { t, errorMessage } from '../lib/i18n.svelte.js';

  let { onAuthenticated } = $props();

  let email = $state('');
  let code = $state('');
  let error = $state('');
  let loading = $state(false);

  async function submit() {
    error = '';
    loading = true;
    try {
      const resp = await api.login({ email, code });
      onAuthenticated(resp);
    } catch (e) {
      error = errorMessage(e.code, e.message);
    } finally {
      loading = false;
    }
  }
</script>

{#if error}
  <div class="error">{error}</div>
{/if}

<form onsubmit={(e) => (e.preventDefault(), submit())}>
  <div class="field">
    <label for="email">{t('common.email')}</label>
    <input id="email" type="email" bind:value={email} autocomplete="email" />
  </div>
  <div class="field">
    <label for="code">{t('signin.code_label')}</label>
    <input id="code" type="text" bind:value={code} inputmode="numeric" />
  </div>
  <button type="submit" disabled={loading}>
    {loading ? t('signin.submitting') : t('signin.submit')}
  </button>
</form>
