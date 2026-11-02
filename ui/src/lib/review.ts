// Pull request review, the parts that are pure: the words for votes and thread states, and how
// threads are grouped by file and by line for the views.

import type {
  Anchor,
  LineSide,
  PullRequest,
  PullRequestQuery,
  Reviewer,
  ReviewError,
  ReviewErrorKind,
  Thread,
  ThreadStatus,
  Vote,
} from "./bindings";

export const QUERIES: { value: PullRequestQuery; label: string }[] = [
  { value: "active", label: "Active" },
  { value: "reviewing", label: "Assigned to me" },
  { value: "mine", label: "Created by me" },
  { value: "completed", label: "Completed" },
  { value: "abandoned", label: "Abandoned" },
];

/** The votes a reviewer can cast, as buttons: approval first. */
export const VOTES: { vote: Vote; label: string }[] = [
  { vote: "approved", label: "Approve" },
  { vote: "approvedWithSuggestions", label: "Approve with suggestions" },
  { vote: "waitingForAuthor", label: "Wait for author" },
  { vote: "rejected", label: "Reject" },
];

const VOTED: Record<Vote, { label: string; mark: string }> = {
  approved: { label: "Approved", mark: "✓" },
  approvedWithSuggestions: { label: "Approved with suggestions", mark: "✓" },
  none: { label: "No vote", mark: "–" },
  waitingForAuthor: { label: "Waiting for author", mark: "…" },
  rejected: { label: "Rejected", mark: "✕" },
};

/** A cast vote in words: "Approved", "No vote". */
export function voteLabel(vote: Vote): string {
  return VOTED[vote].label;
}

/** A one-character mark for a vote, beside a reviewer's name. */
export function voteMark(vote: Vote): string {
  return VOTED[vote].mark;
}

/** The thread states a thread can be set to, in the order offered. */
export const THREAD_STATUSES: { status: ThreadStatus; label: string }[] = [
  { status: "active", label: "Active" },
  { status: "pending", label: "Pending" },
  { status: "fixed", label: "Resolved" },
  { status: "wontFix", label: "Won't fix" },
  { status: "closed", label: "Closed" },
  { status: "byDesign", label: "By design" },
];

export function statusLabel(status: ThreadStatus): string {
  return THREAD_STATUSES.find((entry) => entry.status === status)?.label ?? "Unknown";
}

/** Whether a thread needs nothing more: resolved one way or another. */
export function isResolved(status: ThreadStatus): boolean {
  return status === "fixed" || status === "wontFix" || status === "closed" || status === "byDesign";
}

/** The signed-in user's vote on `pr`; `none` if they aren't a reviewer. */
export function myVote(pr: PullRequest): Vote {
  return pr.summary.reviewers.find((reviewer) => reviewer.person.id === pr.me.id)?.vote ?? "none";
}

/** How many reviewers approved (with or without suggestions), want changes, or rejected. */
export function verdicts(reviewers: readonly Reviewer[]): { approved: number; waiting: number; rejected: number } {
  const count = (votes: Vote[]) => reviewers.filter((reviewer) => votes.includes(reviewer.vote)).length;
  return {
    approved: count(["approved", "approvedWithSuggestions"]),
    waiting: count(["waitingForAuthor"]),
    rejected: count(["rejected"]),
  };
}

/** Threads on the pull request as a whole, oldest first. */
export function generalThreads(threads: readonly Thread[]): Thread[] {
  return threads.filter((thread) => thread.anchor === null);
}

/** Threads attached to `path`: those on the whole file first, then by line, older sides first. */
export function threadsOn(threads: readonly Thread[], path: string): Thread[] {
  return threads
    .filter((thread) => thread.anchor?.path === path)
    .sort((a, b) => a.anchor!.line - b.anchor!.line || sideOrder(a.anchor!.side) - sideOrder(b.anchor!.side));
}

function sideOrder(side: LineSide): number {
  return side === "old" ? 0 : 1;
}

/** Per file path, how many threads are attached to it and how many of those aren't resolved. */
export function threadCounts(threads: readonly Thread[]): Map<string, { total: number; open: number }> {
  const counts = new Map<string, { total: number; open: number }>();
  for (const thread of threads) {
    if (!thread.anchor) continue;
    const count = counts.get(thread.anchor.path) ?? { total: 0, open: 0 };
    count.total++;
    if (!isResolved(thread.status)) count.open++;
    counts.set(thread.anchor.path, count);
  }
  return counts;
}

/** Names a line of one side of a diff: `old:12`, `new:5`. */
export function lineKey(side: LineSide, line: number): string {
  return `${side}:${line}`;
}

/** The threads of `path` attached to lines, by `lineKey`. */
export function threadsByLine(threads: readonly Thread[], path: string): Map<string, Thread[]> {
  const byLine = new Map<string, Thread[]>();
  for (const thread of threads) {
    const anchor = thread.anchor;
    if (anchor?.path !== path || anchor.line === 0) continue;
    const key = lineKey(anchor.side, anchor.line);
    byLine.set(key, [...(byLine.get(key) ?? []), thread]);
  }
  return byLine;
}

/** Where a thread is, in words: "line 12", "line 3 before the changes", "the whole file". */
export function anchorText(anchor: Anchor): string {
  if (anchor.line === 0) return "the whole file";
  return anchor.side === "old" ? `line ${anchor.line} before the changes` : `line ${anchor.line}`;
}

/** Whether `reason` is a `ReviewError` the backend rejected with. */
export function isReviewError(reason: unknown): reason is ReviewError {
  return typeof reason === "object" && reason !== null && "kind" in reason && "message" in reason;
}

/** `reason` as a `ReviewError`, whatever rejected: a bug or a plugin rejects with something else. */
export function asReviewError(reason: unknown): ReviewError {
  if (isReviewError(reason)) return reason;
  const message = reason instanceof Error ? reason.message : typeof reason === "string" ? reason : "Something went wrong.";
  return { kind: "server", message };
}

const RETRY_LABEL: Partial<Record<ReviewErrorKind, string>> = {
  signIn: "Sign in again",
  unreachable: "Try again",
  server: "Try again",
};

/** What a button offering to try again after `error` says; `null` if trying again can't help. */
export function retryLabel(error: ReviewError): string | null {
  return RETRY_LABEL[error.kind] ?? null;
}
