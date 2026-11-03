<!--
  A tab's pull requests, shown in place of the graph: the list on the left, the one selected on the
  right. Opening a file shows its diff between the pull request's base and its latest commit, with
  the file's threads beside it; clicking a line number starts a comment on that line.
-->
<script lang="ts">
  import { tick } from "svelte";
  import type { FileChange, LineSide, PullRequestQuery, Thread } from "./bindings";
  import CommentBox from "./CommentBox.svelte";
  import DiffView from "./DiffView.svelte";
  import { formatLocalTime } from "./format";
  import PullRequestView from "./PullRequestView.svelte";
  import { QUERIES, anchorText, lineKey, threadsByLine, threadsOn, verdicts } from "./review";
  import type { Review } from "./review.svelte";
  import ReviewNotice from "./ReviewNotice.svelte";
  import ThreadView from "./ThreadView.svelte";

  interface Props {
    review: Review;
    /** Back to the graph. */
    onclose: () => void;
  }

  let { review, onclose }: Props = $props();

  let root: HTMLElement;
  let diffView = $state<ReturnType<typeof DiffView>>();
  /** Where a new comment in the open file goes; `line` 0 is the file as a whole. */
  let draft = $state<{ side: LineSide; line: number } | null>(null);
  /** A line to scroll to once the diff opening shows it. */
  let pendingLine: { side: LineSide; line: number } | null = null;

  const list = $derived(review.list);
  const pr = $derived(review.pullRequest.value);
  const path = $derived(review.diff?.request.path ?? null);
  const fileThreads = $derived(pr && path !== null ? threadsOn(pr.threads, path) : []);
  const lineCounts = $derived.by(() => {
    const counts = new Map<string, number>();
    if (pr && path !== null) for (const [key, threads] of threadsByLine(pr.threads, path)) counts.set(key, threads.length);
    return counts;
  });

  // Scroll to the line a thread was opened at, once its diff has loaded.
  $effect(() => {
    if (review.diff?.result && diffView && pendingLine) {
      const { side, line } = pendingLine;
      pendingLine = null;
      void tick().then(() => diffView?.scrollToLine(side, line));
    }
  });

  function openFile(file: FileChange): void {
    draft = null;
    review.openFile(file);
  }

  function openThread(thread: Thread): void {
    const anchor = thread.anchor;
    if (!anchor) return;
    const file = review.files?.find((candidate) => candidate.path === anchor.path) ?? {
      path: anchor.path,
      oldPath: null,
      status: "modified",
      additions: null,
      deletions: null,
    };
    openFile(file);
    pendingLine = anchor.line > 0 ? { side: anchor.side, line: anchor.line } : null;
  }

  function closeDiff(): void {
    draft = null;
    review.closeDiff();
    void tick().then(() => focus());
  }

  async function comment(text: string): Promise<boolean> {
    if (!pr || path === null || !draft) return false;
    const anchor = { path, side: draft.side, line: draft.line };
    const sent = await review.act("comment", { kind: "comment", text, anchor, iteration: pr.iteration });
    if (sent) draft = null;
    return sent;
  }

  /** Focuses what the keyboard works on: the open diff, or else the review. */
  export function focus(): void {
    if (review.diff) diffView?.focus();
    else root?.focus({ preventScroll: true });
  }

  /** The open diff, for the app's key router. */
  export function diff(): ReturnType<typeof DiffView> | undefined {
    return review.diff ? diffView : undefined;
  }

  /** Esc: closes the diff, else the review. */
  export function escape(): void {
    if (review.diff) closeDiff();
    else onclose();
  }
</script>

<div bind:this={root} class="review" tabindex="-1" aria-label="Pull requests">
  <aside class="list">
    <header class="list-header">
      <select
        aria-label="Which pull requests"
        value={review.query}
        onchange={(event) => review.setQuery(event.currentTarget.value as PullRequestQuery)}
      >
        {#each QUERIES as query (query.value)}<option value={query.value}>{query.label}</option>{/each}
      </select>
      <span class="host" title="{review.host?.organization} / {review.host?.project} / {review.host?.repository}">
        {review.host?.project} / {review.host?.repository}
      </span>
      <button class="icon-button" aria-label="Refresh" title="Refresh" disabled={list.loading} onclick={() => void review.loadList()}>↻</button>
    </header>
    {#if list.error}
      <div class="pad"><ReviewNotice error={list.error} onretry={() => void review.loadList()} /></div>
    {/if}
    {#if list.value === undefined}
      {#if list.loading}<p class="note pad">Loading…</p>{/if}
    {:else if list.value.length === 0}
      <p class="note pad">No pull requests.</p>
    {:else}
      <ul class="items">
        {#each list.value as item (item.number)}
          {@const count = verdicts(item.reviewers)}
          <li>
            <button class="item" class:selected={item.number === review.selected} onclick={() => review.select(item.number)}>
              <span class="item-title">
                {#if item.draft}<span class="draft">Draft</span>{/if}
                {item.title}
              </span>
              <span class="item-meta">
                !{item.number} · {item.author.name} · {formatLocalTime(item.createdAt)}
              </span>
              <span class="item-meta mono">{item.sourceBranch} → {item.targetBranch}</span>
              {#if count.approved + count.waiting + count.rejected > 0}
                <span class="item-votes">
                  {#if count.approved}<span class="approved" title="Approved">✓ {count.approved}</span>{/if}
                  {#if count.waiting}<span class="waiting" title="Waiting for author">… {count.waiting}</span>{/if}
                  {#if count.rejected}<span class="rejected" title="Rejected">✕ {count.rejected}</span>{/if}
                </span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </aside>

  <section class="main">
    {#if review.diff}
      <div class="file">
        {#key review.diff.request}
          <DiffView
            bind:this={diffView}
            diff={review.diff}
            onclose={closeDiff}
            back="Pull request"
            threads={lineCounts}
            commenting={draft && draft.line > 0 ? lineKey(draft.side, draft.line) : null}
            onpickline={(side, line) => (draft = { side, line })}
          />
        {/key}
        <aside class="file-threads" aria-label="Comments on this file">
          {#if review.actionError}
            <ReviewNotice error={review.actionError} ondismiss={() => review.dismissActionError()} />
          {/if}
          {#if draft}
            <div class="draft-box">
              <p class="note">
                Comment on {draft.line === 0 ? "the whole file" : anchorText({ path: path ?? "", side: draft.side, line: draft.line })}
              </p>
              {#key draft}
                <CommentBox label="Write a comment" submit="Comment" busy={review.acting.has("comment")} onsubmit={comment} oncancel={() => (draft = null)} autofocus />
              {/key}
            </div>
          {:else}
            <p class="note">Click a line number to comment on that line.</p>
            <button class="button" onclick={() => (draft = { side: "new", line: 0 })}>Comment on the file</button>
          {/if}
          {#each fileThreads as thread (thread.id)}
            <ThreadView {thread} {review} showPath={false} onopen={(t) => t.anchor && diffView?.scrollToLine(t.anchor.side, t.anchor.line)} />
          {/each}
        </aside>
      </div>
    {:else if review.pullRequest.error && !pr}
      <div class="pad"><ReviewNotice error={review.pullRequest.error} onretry={() => void review.reload()} /></div>
    {:else if pr}
      <div class="scroll">
        {#if review.pullRequest.error}
          <div class="pad"><ReviewNotice error={review.pullRequest.error} onretry={() => void review.reload()} /></div>
        {/if}
        <PullRequestView {review} {pr} onopenfile={openFile} onopenthread={openThread} />
      </div>
    {:else if review.selected !== null}
      <p class="note pad">Loading…</p>
    {:else}
      <p class="note pad">Choose a pull request to review.</p>
    {/if}
  </section>
</div>

<style>
  .review {
    display: flex;
    flex: 1;
    min-width: 0;
    min-height: 0;
    outline: none;
  }

  .list {
    display: flex;
    flex: none;
    flex-direction: column;
    width: min(320px, 38%);
    min-height: 0;
    border-right: 1px solid var(--border);
  }

  .list-header {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    height: 36px;
    padding: 0 8px;
    border-bottom: 1px solid var(--border);
    background: var(--bg-subtle);
  }

  select {
    height: 26px;
    border: 1px solid var(--border-strong);
    border-radius: 5px;
    background: var(--button-bg);
    color: var(--fg);
    font: inherit;
  }

  .host {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--fg-muted);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .pad {
    padding: 10px;
  }

  .note {
    margin: 0;
    color: var(--fg-muted);
  }

  .items {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0;
    overflow: auto;
    list-style: none;
  }

  .item {
    display: flex;
    flex-direction: column;
    gap: 2px;
    width: 100%;
    padding: 8px 10px;
    border: 0;
    border-bottom: 1px solid var(--border);
    border-radius: 0;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
  }

  .item:hover {
    background: var(--bg-hover);
  }

  .item.selected {
    background: var(--bg-selected);
  }

  .item-title {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .draft {
    margin-right: 4px;
    padding: 0 5px;
    border: 1px solid var(--border-strong);
    border-radius: 9px;
    color: var(--fg-muted);
    font-size: 11px;
    font-weight: 400;
  }

  .item-meta {
    overflow: hidden;
    color: var(--fg-muted);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mono {
    font-family: var(--font-mono);
  }

  .item-votes {
    display: flex;
    gap: 8px;
    font-size: 12px;
  }

  .approved {
    color: var(--status-added);
  }
  .waiting {
    color: var(--status-modified);
  }
  .rejected {
    color: var(--status-deleted);
  }

  .main {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }

  .file {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .file-threads {
    display: flex;
    flex: none;
    flex-direction: column;
    gap: 10px;
    width: min(360px, 40%);
    padding: 10px;
    overflow: auto;
    border-left: 1px solid var(--border);
    background: var(--bg-subtle);
  }

  .draft-box {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

</style>
