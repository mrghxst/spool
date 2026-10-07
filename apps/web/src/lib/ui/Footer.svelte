<script lang="ts">
  import { REPO_URL, VERSION } from '../links';
</script>

<footer>
  <p class="small muted">
    Credentials stay in this browser. TLS runs end-to-end to your provider. The relay only sees encrypted bytes.
  </p>
  <p class="small muted">
    Spool {VERSION} · Open source on
    <a href={REPO_URL} target="_blank" rel="noopener noreferrer">GitHub</a> · MIT license
  </p>
</footer>

<style>
  footer {
    max-width: 880px;
    margin: 0 auto;
    padding: 24px 16px 32px;
    display: grid;
    gap: 4px;
  }
  a {
    text-underline-offset: 2px;
  }
  a:hover {
    color: var(--fg);
  }
</style>
