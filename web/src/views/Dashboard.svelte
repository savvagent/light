<script>
  let { user, onLogout } = $props();
</script>

<div class="dashboard">
  <p class="subtitle">You're logged in — create something great.</p>

  <dl>
    <dt>Email</dt>
    <dd>{user.email}</dd>

    <dt>Display name</dt>
    <dd>{user.display_name}</dd>

    <dt>Sign-in method</dt>
    <dd>Authenticator app</dd>
  </dl>

  <button class="secondary" onclick={onLogout}>Sign out</button>
</div>
