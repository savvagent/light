<script>
  import { api } from '../lib/api.js';

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
      error = e.message;
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
    <label for="email">Email</label>
    <input id="email" type="email" bind:value={email} autocomplete="email" />
  </div>
  <div class="field">
    <label for="name">Display name (optional)</label>
    <input id="name" type="text" bind:value={displayName} />
  </div>
  <button type="submit" disabled={loading}>
    {loading ? 'Creating account…' : 'Create account'}
  </button>
</form>
