<!--
  A text box for a comment or reply, with a button that sends it (Ctrl+Enter too). The text is
  cleared once `onsubmit` reports it was sent, and kept if not, so nothing typed is lost.
-->
<script lang="ts">
  import { onMount } from "svelte";

  interface Props {
    label: string;
    /** What the sending button says. */
    submit: string;
    /** A request with this box's text is on its way. */
    busy: boolean;
    /** Sends the text; resolves with whether it was sent. */
    onsubmit: (text: string) => Promise<boolean>;
    /** Shows a Cancel button. */
    oncancel?: () => void;
    autofocus?: boolean;
  }

  let { label, submit, busy, onsubmit, oncancel, autofocus = false }: Props = $props();

  let text = $state("");
  let field: HTMLTextAreaElement;

  onMount(() => {
    if (autofocus) field.focus();
  });

  async function send(): Promise<void> {
    if (busy || text.trim() === "") return;
    if (await onsubmit(text)) text = "";
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void send();
    } else if (event.key === "Escape" && oncancel) {
      event.preventDefault();
      oncancel();
    }
  }
</script>

<div class="comment-box">
  <textarea bind:this={field} bind:value={text} aria-label={label} placeholder={label} rows="3" {onkeydown}></textarea>
  <div class="actions">
    {#if oncancel}<button class="button" onclick={oncancel}>Cancel</button>{/if}
    <button class="button primary" disabled={busy || text.trim() === ""} onclick={() => void send()} title="{submit} (Ctrl+Enter)">
      {busy ? "Sending…" : submit}
    </button>
  </div>
</div>

<style>
  .comment-box {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  textarea {
    box-sizing: border-box;
    width: 100%;
    min-height: 60px;
    padding: 6px 8px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--bg);
    color: var(--fg);
    font: inherit;
    resize: vertical;
  }

  textarea:focus {
    border-color: var(--focus);
    outline: none;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
  }
</style>
