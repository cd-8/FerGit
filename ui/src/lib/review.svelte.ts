import type {
  FileChange,
  OpOutcome,
  Operation,
  PullRequest,
  PullRequestQuery,
  PullRequestSummary,
  ReviewAction,
  ReviewError,
  ReviewHost,
} from "./bindings";
import type { RepoClient } from "./client";
import { commitSide, type DiffRequest, type OpenDiff } from "./inspector.svelte";
import { asReviewError } from "./review";

/** What a request last answered, and whether another is on its way or the last one failed. */
export interface Loaded<T> {
  /** The latest answer; kept on screen while a newer request is on its way. */
  value: T | undefined;
  loading: boolean;
  error: ReviewError | null;
}

const nothing = <T>(): Loaded<T> => ({ value: undefined, loading: false, error: null });

type ReviewClient = Pick<RepoClient, "reviewHost" | "pullRequests" | "pullRequest" | "review" | "fileDiff" | "changes">;

/** How the review runs a git operation: through the tab's operations, so it shows like any other. */
export interface OperationRunner {
  run(op: Operation, title: string): Promise<OpOutcome>;
}

/**
 * One tab's pull requests: whether its repository has any (an Azure DevOps remote), the list, the
 * pull request selected with the files it changes, the file diff open from it, and the actions on
 * their way.
 *
 * Nothing here is patched after an action: the pull request is read again from the host, as the
 * graph is read again from git after an operation. Only the latest request of each kind is used;
 * answers about a pull request no longer selected are dropped. Talking to the host happens only once
 * the user opens the review, since signing in may run credential helpers and prompt. Reads are
 * reactive.
 */
export class Review {
  /** Where the pull requests are; `null` if no remote is on Azure DevOps (or not known yet). */
  host = $state.raw<ReviewHost | null>(null);
  /** Whether the review is shown in place of the graph. */
  open = $state(false);
  query = $state<PullRequestQuery>("active");
  list = $state.raw<Loaded<PullRequestSummary[]>>(nothing());
  selected = $state.raw<number | null>(null);
  pullRequest = $state.raw<Loaded<PullRequest>>(nothing());
  /** What the selected pull request changes, once its commits are here; `undefined` until then. */
  files = $state.raw<FileChange[] | undefined>(undefined);
  /** The file diff opened from the pull request. */
  diff = $state.raw<OpenDiff | null>(null);
  /** What actions on their way act on: keys the views choose ("vote", "reply:9"). */
  acting = $state.raw<ReadonlySet<string>>(new Set());
  /** Why the last action failed, shown until the next one starts or it is dismissed. */
  actionError = $state.raw<ReviewError | null>(null);
  /** Whether the pull request's commits are being fetched. */
  fetching = $state(false);

  readonly #client: ReviewClient;
  readonly #operations: OperationRunner;
  #listToken = 0;
  #pullRequestToken = 0;
  #diffToken = 0;
  /** `base..head` of the files shown, to read them again only when those change. */
  #filesFor = "";

  constructor(client: ReviewClient, operations: OperationRunner) {
    this.#client = client;
    this.#operations = operations;
  }

  /** Finds out whether the repository has pull requests to review. Contacts nothing. */
  async findHost(): Promise<void> {
    this.host = await this.#client.reviewHost();
  }

  /** Shows or hides the review; the first time it is shown, its list loads. */
  toggle(open = !this.open): void {
    this.open = open && this.host !== null;
    const list = this.list;
    if (this.open && list.value === undefined && !list.loading) void this.loadList();
  }

  setQuery(query: PullRequestQuery): void {
    this.query = query;
    void this.loadList();
  }

  /** Reads the list again, keeping the current one on screen meanwhile. */
  async loadList(): Promise<void> {
    const token = ++this.#listToken;
    this.list = { ...this.list, loading: true, error: null };
    try {
      const value = await this.#client.pullRequests(this.query);
      if (token === this.#listToken) this.list = { value, loading: false, error: null };
    } catch (reason) {
      if (token === this.#listToken) this.list = { ...this.list, loading: false, error: asReviewError(reason) };
    }
  }

  /** Selects pull request `number` and reads it. */
  select(number: number): void {
    if (number === this.selected && this.pullRequest.value) return;
    this.selected = number;
    this.pullRequest = nothing();
    this.files = undefined;
    this.#filesFor = "";
    this.actionError = null;
    this.closeDiff();
    void this.reload();
  }

  /**
   * Reads the selected pull request again (keeping what is shown until the answer arrives), and its
   * files if its commits are here and moved, or arrived.
   */
  async reload(): Promise<void> {
    const number = this.selected;
    if (number === null) return;
    const token = ++this.#pullRequestToken;
    this.pullRequest = { ...this.pullRequest, loading: true, error: null };
    let pr: PullRequest;
    try {
      pr = await this.#client.pullRequest(number);
    } catch (reason) {
      if (token === this.#pullRequestToken) this.pullRequest = { ...this.pullRequest, loading: false, error: asReviewError(reason) };
      return;
    }
    if (token !== this.#pullRequestToken) return;
    this.pullRequest = { value: pr, loading: false, error: null };
    const range = pr.haveCommits && pr.base && pr.head ? `${pr.base}..${pr.head}` : "";
    if (range === this.#filesFor) return;
    this.#filesFor = range;
    this.files = undefined;
    if (range === "") return;
    const files = await this.#client.changes(commitSide(pr.base!), commitSide(pr.head!));
    if (token === this.#pullRequestToken) this.files = files;
  }

  /**
   * Does `action` to the selected pull request, as long as nothing else with the same `key` is on
   * its way, then reads the pull request again. Resolves with whether it was done; why not is in
   * `actionError`.
   */
  async act(key: string, action: ReviewAction): Promise<boolean> {
    const number = this.selected;
    if (number === null || this.acting.has(key)) return false;
    this.acting = new Set([...this.acting, key]);
    this.actionError = null;
    try {
      await this.#client.review(crypto.randomUUID(), number, action);
    } catch (reason) {
      this.actionError = asReviewError(reason);
      return false;
    } finally {
      this.acting = new Set([...this.acting].filter((other) => other !== key));
    }
    if (number === this.selected) void this.reload();
    // The list shows votes too.
    if (action.kind === "vote") void this.loadList();
    return true;
  }

  dismissActionError(): void {
    this.actionError = null;
  }

  /** Fetches the selected pull request's commits from the host's remote, then reads it again. */
  async fetchCommits(): Promise<void> {
    const pr = this.pullRequest.value;
    const host = this.host;
    if (!pr?.head || !host || this.fetching) return;
    this.fetching = true;
    try {
      const op: Operation = {
        kind: "fetchPullRequest",
        remote: host.remote,
        number: pr.summary.number,
        branch: pr.summary.sourceBranch,
        head: pr.head,
      };
      await this.#operations.run(op, `Fetching pull request !${pr.summary.number}`);
    } finally {
      this.fetching = false;
    }
    await this.reload();
  }

  /** Opens the diff of `file` between the pull request's base and its latest commit. */
  openFile(file: FileChange): void {
    const pr = this.pullRequest.value;
    if (!pr?.base || !pr.head) return;
    const request: DiffRequest = {
      list: "range",
      from: commitSide(pr.base),
      to: commitSide(pr.head),
      path: file.path,
      oldPath: file.oldPath,
    };
    const token = ++this.#diffToken;
    this.diff = { request, result: undefined };
    void this.#client.fileDiff(request.from, request.to, request.path, request.oldPath).then((result) => {
      if (token === this.#diffToken) this.diff = { request, result };
    });
  }

  closeDiff(): void {
    this.#diffToken++;
    this.diff = null;
  }
}
