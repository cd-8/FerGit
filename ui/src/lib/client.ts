import {
  commands,
  type CommitDetails,
  type DiffSide,
  type FileChange,
  type FileDiff,
  type Filter,
  type OpId,
  type OpOutcome,
  type Oid,
  type Operation,
  type PullRequest,
  type PullRequestQuery,
  type PullRequestSummary,
  type RefLabel,
  type RepoInfo,
  type ReviewAction,
  type ReviewHost,
  type RowLocation,
  type RowsPage,
  type SearchResult,
  type SessionId,
  type Undoable,
} from "./bindings";

/**
 * The commands of one open repository (one tab), bound to its backend session. Each tab's views
 * talk to the backend only through its client, so none of them needs to know which session it is.
 */
export interface RepoClient {
  refresh(): Promise<RepoInfo>;
  rows(start: number, len: number): Promise<RowsPage>;
  locate(id: Oid): Promise<RowLocation>;
  commitDetails(id: Oid): Promise<CommitDetails | null>;
  changes(from: DiffSide | null, to: DiffSide): Promise<FileChange[]>;
  fileDiff(from: DiffSide | null, to: DiffSide, path: string, oldPath: string | null): Promise<FileDiff>;
  /** The rows of the current snapshot that `query` finds. */
  search(query: string): Promise<SearchResult>;
  /** Shows only the commits `filter` selects; answers like `refresh`. */
  setFilter(filter: Filter): Promise<RepoInfo>;
  /** Every ref of the repository, for choosing what to filter on. */
  refs(): Promise<RefLabel[]>;
  /** Runs `op` as request `id`; progress arrives as `opProgress` events naming this session. */
  runOperation(id: OpId, op: Operation): Promise<OpOutcome>;
  /** What undo would restore now in this repository. */
  undoable(): Promise<Undoable>;
  /** Where the repository's pull requests are (no network); `null` if no remote is on Azure DevOps. */
  reviewHost(): Promise<ReviewHost | null>;
  /**
   * Pull requests, one of them, and acting on one as request `id`. These talk to the host and may
   * prompt for credentials; they reject with a `ReviewError`, which the review view shows in place.
   */
  pullRequests(query: PullRequestQuery): Promise<PullRequestSummary[]>;
  pullRequest(number: number): Promise<PullRequest>;
  review(id: OpId, number: number, action: ReviewAction): Promise<void>;
  /**
   * Closes the session. From then on the client drops requests, and answers still on their way:
   * their promises never settle, so nothing acts on a closed tab and a request that raced the close
   * can't raise an error about it.
   */
  close(): Promise<void>;
}

export function repoClient(session: SessionId): RepoClient {
  let closed = false;
  const call = <T>(request: () => Promise<T>): Promise<T> => {
    if (closed) return never();
    return request().then(
      (value) => (closed ? never<T>() : value),
      (error: unknown) => (closed ? never<T>() : Promise.reject(error)),
    );
  };
  return {
    refresh: () => call(() => commands.refresh(session)),
    rows: (start, len) => call(() => commands.rows(session, start, len)),
    locate: (id) => call(() => commands.locate(session, id)),
    commitDetails: (id) => call(() => commands.commitDetails(session, id)),
    changes: (from, to) => call(() => commands.changes(session, from, to)),
    fileDiff: (from, to, path, oldPath) => call(() => commands.fileDiff(session, from, to, path, oldPath)),
    search: (query) => call(() => commands.search(session, query)),
    setFilter: (filter) => call(() => commands.setFilter(session, filter)),
    refs: () => call(() => commands.refs(session)),
    runOperation: (id, op) => call(() => commands.runOperation(session, id, op)),
    undoable: () => call(() => commands.undoable(session)),
    reviewHost: () => call(() => commands.reviewHost(session)),
    pullRequests: (query) => call(() => commands.pullRequests(session, query)),
    pullRequest: (number) => call(() => commands.pullRequest(session, number)),
    review: (id, number, action) => call(() => commands.review(session, id, number, action).then(() => undefined)),
    close: async () => {
      if (closed) return;
      closed = true;
      await commands.closeRepo(session);
    },
  };
}

function never<T>(): Promise<T> {
  return new Promise<T>(() => {});
}
