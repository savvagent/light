<script>
  import { api } from '../lib/api.js';
  import { t, errorMessage } from '../lib/i18n.svelte.js';

  let { onRegistered } = $props();

  let email = $state('');
  let displayName = $state('');
  let error = $state('');
  let loading = $state(false);

  async function submit() {
    error = '';
    loading = true;
    try {
      const resp = await api.register({
        email,
        display_name: displayName || undefined,
      });
      onRegistered(resp);
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
    <label for="name">{t('signup.name_label')}</label>
    <input id="name" type="text" bind:value={displayName} />
  </div>
  <button type="submit" disabled={loading}>
    {loading ? t('signup.submitting') : t('signup.submit')}
  </button>
</form>
