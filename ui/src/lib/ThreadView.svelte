<!--
  One comment thread: where it is (if asked), its comments, its state with a way to change it, and a
  reply box. Comments are Markdown as written, shown as plain text.
-->
<script lang="ts">
  import type { Thread, ThreadStatus } from "./bindings";
  import CommentBox from "./CommentBox.svelte";
  import { formatLocalTime } from "./format";
  import { THREAD_STATUSES, anchorText, isResolved, statusLabel } from "./review";
  import type { Review } from "./review.svelte";

  interface Props {
    thread: Thread;
    review: Review;
    /** Makes where the thread is a link, which calls this. */
    onopen?: (thread: Thread) => void;
    /** Whether `onopen` can open it now (the pull request's commits are here). */
    openable?: boolean;
    /** Say which file the thread is on; not where that file is open already. */
    showPath?: boolean;
  }

  let { thread, review, onopen, openable = true, showPath = true }: Props = $props();

  let replying = $state(false);
  const resolved = $derived(isResolved(thread.status));
  const root = $derived(thread.comments[0]);
  const replyKey = $derived(`reply:${thread.id}`);
  const statusKey = $derived(`status:${thread.id}`);

  function setStatus(status: ThreadStatus): void {
    void review.act(statusKey, { kind: "setStatus", thread: thread.id, status });
  }

  async function reply(text: string): Promise<boolean> {
    const sent = await review.act(replyKey, { kind: "reply", thread: thread.id, parent: root.id, text });
    if (sent) replying = false;
    return sent;
  }
</script>

<article class="thread" class:resolved>
  <header class="thread-header">
    {#if thread.anchor}
      {@const anchor = thread.anchor}
      {@const line = anchor.line > 0 ? anchorText(anchor) : ""}
      {#if onopen && (showPath || anchor.line > 0)}
        <button class="link" disabled={!openable} onclick={() => onopen(thread)} title={showPath ? "Open the file here" : "Go to the line"}>
          {#if showPath}<span class="mono">{anchor.path}</span>{line ? `, ${line}` : ""}{:else}{line}{/if}
        </button>
      {:else}
        <span>{showPath ? anchor.path : anchorText(anchor)}</span>
      {/if}
    {/if}
    <span class="spacer"></span>
    {#if thread.status === "unknown"}
      <span class="status">{statusLabel(thread.status)}</span>
    {:else}
      <select
        class="status"
        aria-label="Thread state"
        value={thread.status}
        disabled={review.acting.has(statusKey)}
        onchange={(event) => setStatus(event.currentTarget.value as ThreadStatus)}
      >
        {#each THREAD_STATUSES as entry (entry.status)}
          <option value={entry.status}>{entry.label}</option>
        {/each}
      </select>
    {/if}
  </header>
  {#each thread.comments as comment (comment.id)}
    <div class="comment" class:reply={comment.parent !== 0}>
      <div class="byline">
        <span class="author" title={comment.author.uniqueName}>{comment.author.name}</span>
        <span class="when">{formatLocalTime(comment.publishedAt)}{comment.edited ? " · edited" : ""}</span>
      </div>
      <p class="text">{comment.text}</p>
    </div>
  {/each}
  {#if replying}
    <CommentBox label="Reply" submit="Reply" busy={review.acting.has(replyKey)} onsubmit={reply} oncancel={() => (replying = false)} autofocus />
  {:else}
    <div class="thread-actions">
      <button class="button" onclick={() => (replying = true)}>Reply</button>
      {#if !resolved && thread.status !== "unknown"}
        <button class="button" disabled={review.acting.has(statusKey)} onclick={() => setStatus("fixed")}>Resolve</button>
      {/if}
    </div>
  {/if}
</article>

<style>
  .thread {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
  }

  .thread.resolved {
    background: var(--bg-subtle);
  }

  .thread.resolved .comment {
    opacity: 0.75;
  }

  .thread-header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
    color: var(--fg-muted);
    font-size: 12px;
  }

  .spacer {
    flex: 1;
  }

  .link {
    min-width: 0;
    overflow: hidden;
    padding: 0;
    border: 0;
    background: none;
    color: var(--focus);
    font: inherit;
    text-align: left;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .link:hover:not(:disabled) {
    text-decoration: underline;
  }

  .link:disabled {
    color: var(--fg-muted);
  }

  .mono {
    font-family: var(--font-mono);
  }

  select.status {
    height: 24px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--button-bg);
    color: var(--fg);
    font: inherit;
  }

  .comment.reply {
    padding-left: 12px;
    border-left: 2px solid var(--border);
  }

  .byline {
    display: flex;
    gap: 8px;
    align-items: baseline;
  }

  .author {
    font-weight: 600;
  }

  .when {
    color: var(--fg-subtle);
    font-size: 12px;
  }

  .text {
    margin: 2px 0 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .thread-actions {
    display: flex;
    gap: 6px;
  }
</style>
