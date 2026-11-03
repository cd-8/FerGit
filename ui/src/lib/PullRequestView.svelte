<!--
  One pull request: what it merges, the signed-in user's vote, its reviewers and description, then
  either its discussion (with a box for a new comment) or the files it changes. Clicking a file, or
  where a thread is, opens that file's diff.
-->
<script lang="ts">
  import type { ChangeStatus, FileChange, PullRequest, Thread, Vote } from "./bindings";
  import CommentBox from "./CommentBox.svelte";
  import { formatLocalTime } from "./format";
  import { VOTES, generalThreads, myVote, threadCounts, threadsOn, voteLabel, voteMark } from "./review";
  import type { Review } from "./review.svelte";
  import ReviewNotice from "./ReviewNotice.svelte";
  import ThreadView from "./ThreadView.svelte";

  interface Props {
    review: Review;
    pr: PullRequest;
    onopenfile: (file: FileChange) => void;
    onopenthread: (thread: Thread) => void;
  }

  let { review, pr, onopenfile, onopenthread }: Props = $props();

  const STATUS_LETTER: Record<ChangeStatus, string> = {
    added: "A",
    modified: "M",
    deleted: "D",
    renamed: "R",
    copied: "C",
    typeChanged: "T",
  };

  let section = $state<"discussion" | "files">("discussion");
  let copied = $state(false);

  const summary = $derived(pr.summary);
  const mine = $derived(myVote(pr));
  const general = $derived(generalThreads(pr.threads));
  /** Files with threads, in path order, each with its threads. */
  const fileThreads = $derived(
    [...new Set(pr.threads.flatMap((thread) => (thread.anchor ? [thread.anchor.path] : [])))]
      .sort()
      .map((path) => ({ path, threads: threadsOn(pr.threads, path) })),
  );
  const counts = $derived(threadCounts(pr.threads));
  const active = $derived(summary.status === "active");

  function vote(vote: Vote): void {
    // Clicking the vote already cast takes it back.
    void review.act("vote", { kind: "vote", vote: vote === mine ? "none" : vote });
  }

  function comment(text: string): Promise<boolean> {
    return review.act("comment", { kind: "comment", text, anchor: null, iteration: pr.iteration });
  }

  async function copyLink(): Promise<void> {
    await navigator.clipboard.writeText(pr.webUrl);
    copied = true;
    setTimeout(() => (copied = false), 1500);
  }
</script>

<div class="pull-request">
  <header class="heading">
    <h1 class="title">{summary.title} <span class="number">!{summary.number}</span></h1>
    <div class="heading-actions">
      <button class="button" onclick={() => void copyLink()} title={pr.webUrl}>{copied ? "Copied" : "Copy link"}</button>
      <button class="button" disabled={review.pullRequest.loading} onclick={() => void review.reload()} title="Read it again from Azure DevOps">
        Refresh
      </button>
    </div>
  </header>
  <p class="meta">
    {#if summary.draft}<span class="badge">Draft</span>{/if}
    {#if !active}<span class="badge">{summary.status === "completed" ? "Completed" : "Abandoned"}</span>{/if}
    {#if active && pr.mergeStatus === "conflicts"}<span class="badge warn">Conflicts</span>{/if}
    <span title={summary.author.uniqueName}>{summary.author.name}</span> wants to merge
    <span class="mono">{summary.sourceBranch}</span> into <span class="mono">{summary.targetBranch}</span>
    · {formatLocalTime(summary.createdAt)}
  </p>

  {#if active}
    <div class="votes" role="group" aria-label="Your vote">
      {#each VOTES as option (option.vote)}
        <button
          class="button vote {option.vote}"
          class:chosen={mine === option.vote}
          aria-pressed={mine === option.vote}
          disabled={review.acting.has("vote")}
          onclick={() => vote(option.vote)}
          title={mine === option.vote ? "Take back your vote" : option.label}
        >
          {option.label}
        </button>
      {/each}
    </div>
  {/if}
  {#if review.actionError}
    <!-- Sticky: the comment box that failed may be far down the discussion. -->
    <div class="sticky">
      <ReviewNotice error={review.actionError} ondismiss={() => review.dismissActionError()} />
    </div>
  {/if}

  <h2>Reviewers</h2>
  {#if summary.reviewers.length === 0}
    <p class="note">No reviewers.</p>
  {:else}
    <ul class="reviewers">
      {#each summary.reviewers as reviewer (reviewer.person.id)}
        <li title={reviewer.person.uniqueName}>
          <span class="vote-mark {reviewer.vote}" aria-hidden="true">{voteMark(reviewer.vote)}</span>
          <span class="name">{reviewer.person.name}</span>
          <span class="note">
            {voteLabel(reviewer.vote)}{reviewer.required ? " · required" : ""}{reviewer.group ? " · group" : ""}{reviewer.declined ? " · declined" : ""}
          </span>
        </li>
      {/each}
    </ul>
  {/if}

  {#if pr.description.trim()}
    <h2>Description</h2>
    <p class="text">{pr.description}</p>
  {/if}

  <div class="sections" role="tablist">
    <button role="tab" class="section" aria-selected={section === "discussion"} onclick={() => (section = "discussion")}>
      Discussion <span class="count">{pr.threads.length}</span>
    </button>
    <button role="tab" class="section" aria-selected={section === "files"} onclick={() => (section = "files")}>
      Files {#if review.files}<span class="count">{review.files.length}</span>{/if}
    </button>
  </div>

  {#if section === "discussion"}
    <div class="threads">
      {#each general as thread (thread.id)}
        <ThreadView {thread} {review} />
      {/each}
      {#each fileThreads as file (file.path)}
        {#each file.threads as thread (thread.id)}
          <ThreadView {thread} {review} onopen={onopenthread} openable={pr.haveCommits} />
        {/each}
      {/each}
      {#if pr.threads.length === 0}<p class="note">No comments yet.</p>{/if}
      <CommentBox label="Add a comment" submit="Comment" busy={review.acting.has("comment")} onsubmit={comment} />
    </div>
  {:else if !pr.haveCommits}
    <div class="fetch">
      <p>
        {#if pr.head}
          This pull request's commits aren't in this repository yet. Fetch them to see its changes and comment on lines.
        {:else}
          Azure DevOps hasn't worked out this pull request's changes yet. Refresh in a moment.
        {/if}
      </p>
      {#if pr.head && review.host}
        <button class="button primary" disabled={review.fetching} onclick={() => void review.fetchCommits()}>
          {review.fetching ? "Fetching…" : `Fetch from ${review.host.remote}`}
        </button>
      {/if}
    </div>
  {:else if review.files === undefined}
    <p class="note">Loading…</p>
  {:else if review.files.length === 0}
    <p class="note">No changes.</p>
  {:else}
    <ul class="files">
      {#each review.files as file (file.path)}
        {@const count = counts.get(file.path)}
        <li>
          <button class="file" onclick={() => onopenfile(file)} title={file.oldPath ? `${file.oldPath} → ${file.path}` : file.path}>
            <span class="status {file.status}">{STATUS_LETTER[file.status]}</span>
            <span class="path">{file.path}</span>
            {#if count}<span class="threads-count" title="{count.open} of {count.total} threads not resolved">💬 {count.open}/{count.total}</span>{/if}
            {#if file.additions !== null}<span class="added">+{file.additions}</span>{/if}
            {#if file.deletions !== null}<span class="removed">−{file.deletions}</span>{/if}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>

<style>
  .pull-request {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 920px;
    padding: 14px 18px 24px;
  }

  .heading {
    display: flex;
    align-items: flex-start;
    gap: 10px;
  }

  .title {
    flex: 1;
    min-width: 0;
    margin: 0;
    font-size: 18px;
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .number {
    color: var(--fg-subtle);
    font-weight: 400;
  }

  .heading-actions {
    display: flex;
    flex: none;
    gap: 6px;
  }

  .meta {
    margin: 0;
    color: var(--fg-muted);
  }

  .badge {
    display: inline-block;
    margin-right: 6px;
    padding: 0 6px;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    font-size: 11px;
  }

  .badge.warn {
    border-color: var(--danger-border);
    background: var(--danger-bg);
    color: var(--danger-fg);
  }

  .mono {
    font-family: var(--font-mono);
    font-size: 12px;
  }

  .sticky {
    position: sticky;
    top: 8px;
    z-index: 1;
  }

  .votes {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .vote.chosen {
    border-color: transparent;
    color: var(--primary-fg);
  }

  .vote.chosen.approved,
  .vote.chosen.approvedWithSuggestions {
    background: var(--status-added);
  }

  .vote.chosen.waitingForAuthor {
    background: var(--status-modified);
  }

  .vote.chosen.rejected {
    background: var(--status-deleted);
  }

  h2 {
    margin: 6px 0 0;
    color: var(--fg-muted);
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .note {
    margin: 0;
    color: var(--fg-muted);
  }

  .reviewers {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .reviewers li {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .vote-mark {
    width: 1.2em;
    font-weight: 700;
    text-align: center;
  }

  .vote-mark.approved,
  .vote-mark.approvedWithSuggestions {
    color: var(--status-added);
  }

  .vote-mark.waitingForAuthor {
    color: var(--status-modified);
  }

  .vote-mark.rejected {
    color: var(--status-deleted);
  }

  .vote-mark.none {
    color: var(--fg-subtle);
  }

  .text {
    margin: 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .sections {
    display: flex;
    gap: 2px;
    margin-top: 6px;
    border-bottom: 1px solid var(--border);
  }

  .section {
    padding: 6px 12px;
    border: 0;
    border-bottom: 2px solid transparent;
    background: none;
    color: var(--fg-muted);
    font: inherit;
  }

  .section[aria-selected="true"] {
    border-bottom-color: var(--focus);
    color: var(--fg);
  }

  .count {
    margin-left: 4px;
    padding: 0 6px;
    border-radius: 9px;
    background: var(--bg-subtle);
    font-size: 11px;
  }

  .threads {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .fetch {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .fetch p {
    margin: 0;
  }

  .files {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 26px;
    padding: 0 6px;
    border: 0;
    border-radius: 4px;
    background: none;
    color: var(--fg);
    font: inherit;
    text-align: left;
  }

  .file:hover {
    background: var(--bg-hover);
  }

  .file .path {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-mono);
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .file .status {
    width: 1.2em;
    font-family: var(--font-mono);
    font-weight: 700;
  }

  .status.added {
    color: var(--status-added);
  }
  .status.modified {
    color: var(--status-modified);
  }
  .status.deleted {
    color: var(--status-deleted);
  }
  .status.renamed {
    color: var(--status-renamed);
  }
  .status.copied {
    color: var(--status-copied);
  }
  .status.typeChanged {
    color: var(--status-type);
  }

  .threads-count {
    color: var(--fg-muted);
    font-size: 12px;
  }

  .added {
    color: var(--status-added);
    font-size: 12px;
  }

  .removed {
    color: var(--status-deleted);
    font-size: 12px;
  }
</style>
