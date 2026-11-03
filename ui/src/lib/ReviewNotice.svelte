<!--
  Why talking to Azure DevOps failed, in place: with a way to try again where that can help (signing
  in again, a server that couldn't be reached), and to dismiss it.
-->
<script lang="ts">
  import type { ReviewError } from "./bindings";
  import { retryLabel } from "./review";

  interface Props {
    error: ReviewError;
    onretry?: () => void;
    ondismiss?: () => void;
  }

  let { error, onretry, ondismiss }: Props = $props();
  const retry = $derived(onretry ? retryLabel(error) : null);
</script>

<div class="notice" role="alert">
  <span class="message">{error.message}</span>
  {#if retry}<button class="button" onclick={onretry}>{retry}</button>{/if}
  {#if ondismiss}
    <button class="icon-button" aria-label="Dismiss" title="Dismiss" onclick={ondismiss}>×</button>
  {/if}
</div>

<style>
  .notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--danger-border);
    border-radius: 5px;
    background: var(--danger-bg);
    color: var(--danger-fg);
  }

  .message {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
</style>
