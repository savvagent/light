<script>
  import { onMount } from 'svelte';
  import QRCode from 'qrcode';
  import { api } from '../lib/api.js';

  let { registration, onTotpConfirmed } = $props();

  let qrDataUrl = $state('');
  let code = $state('');
  let error = $state('');
  let loading = $state(true);
  let confirming = $state(false);

  onMount(async () => {
    try {
      qrDataUrl = await QRCode.toDataURL(registration.otpauth_url, {
        width: 220,
        margin: 1,
      });
    } catch (e) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  async function confirm() {
    error = '';
    confirming = true;
    try {
      const resp = await api.registerConfirm({
        setup_token: registration.setup_token,
        code,
      });
      onTotpConfirmed(resp);
    } catch (e) {
      error = e.message;
    } finally {
      confirming = false;
    }
  }
</script>

{#if error}
  <div class="error">{error}</div>
{/if}

{#if loading}
  <p class="subtitle">Generating your authenticator setup…</p>
{:else}
  <p class="subtitle" style="margin-bottom: 8px">
    Scan this QR code with your authenticator app (Google Authenticator,
    1Password, etc.), then enter the code it shows.
  </p>

  <div class="qr">
    <img src={qrDataUrl} alt="TOTP QR code" />
  </div>

  <p class="subtitle" style="text-align: center; margin-bottom: 4px">
    Or enter this secret manually:
  </p>
  <div class="secret">{registration.secret}</div>

  <form onsubmit={(e) => (e.preventDefault(), confirm())}>
    <div class="field">
      <label for="code">Confirmation code</label>
      <input id="code" type="text" bind:value={code} inputmode="numeric" />
    </div>
    <button type="submit" disabled={confirming}>
      {confirming ? 'Verifying…' : 'Confirm and continue'}
    </button>
  </form>
{/if}
