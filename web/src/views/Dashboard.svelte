<script>
  import { t } from '../lib/i18n.svelte.js';

  let { user, onLogout } = $props();
</script>

<div class="dashboard">
  <p class="subtitle">{t('dashboard.subtitle')}</p>

  <dl>
    <dt>{t('dashboard.email')}</dt>
    <dd>{user.email}</dd>

    <dt>{t('dashboard.display_name')}</dt>
    <dd>{user.display_name}</dd>

    <dt>{t('dashboard.signin_method')}</dt>
    <dd>{t('dashboard.method_authenticator')}</dd>
  </dl>

  <button class="secondary" onclick={onLogout}>{t('dashboard.sign_out')}</button>
</div>
