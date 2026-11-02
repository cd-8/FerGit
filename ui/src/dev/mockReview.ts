// Dev-only: a pretend Azure DevOps for the mock page. Three pull requests: one to review with
// reviewers and comment threads, a draft whose commits haven't been fetched (fetching it works), and
// a completed one. Votes, comments, replies and thread states change what the next read returns,
// and a comment containing "fail" is refused, to show how failures look. The third mock repository
// has no Azure DevOps remote.

import type {
  Anchor,
  FileChange,
  FileDiff,
  Hunk,
  Oid,
  Operation,
  Person,
  PullRequest,
  PullRequestQuery,
  PullRequestSummary,
  ReviewAction,
  ReviewError,
  ReviewHost,
  Thread,
  Vote,
} from "../lib/bindings";

const ME: Person = { id: "me", name: "Ada Reviewer", uniqueName: "ada@contoso.com" };
const BO: Person = { id: "bo", name: "Bo Author", uniqueName: "bo@contoso.com" };
const CAROL: Person = { id: "carol", name: "Carol Lead", uniqueName: "carol@contoso.com" };
const TEAM: Person = { id: "team", name: "[Web]\\Web Team", uniqueName: "vstfs:///Classification/TeamProject/web" };

const HOUR = 3600;
const NOW = 1_793_600_000;

interface MockPr {
  summary: PullRequestSummary;
  description: string;
  threads: Thread[];
  fetched: boolean;
}

const FILES: FileChange[] = [
  { path: "README.md", oldPath: null, status: "modified", additions: 1, deletions: 1 },
  { path: "src/form.css", oldPath: null, status: "added", additions: 6, deletions: null },
  { path: "src/login.ts", oldPath: null, status: "modified", additions: 5, deletions: 1 },
];

function comment(id: number, parent: number, author: Person, text: string, at: number) {
  return { id, parent, author, text, publishedAt: at, edited: false };
}

function thread(id: number, anchor: Anchor | null, status: Thread["status"], comments: Thread["comments"]): Thread {
  return { id, status, anchor, comments, updatedAt: comments.at(-1)?.publishedAt ?? NOW };
}

function summary(number: number, title: string, author: Person, status: PullRequestSummary["status"], draft: boolean, source: string): PullRequestSummary {
  return {
    number,
    title,
    author,
    createdAt: NOW - number * HOUR,
    status,
    draft,
    sourceBranch: source,
    targetBranch: "main",
    reviewers: [],
  };
}

function initial(): MockPr[] {
  const login: MockPr = {
    summary: {
      ...summary(42, "Add a login form", BO, "active", false, "feature/login"),
      reviewers: [
        { person: CAROL, vote: "approved", required: true, group: false, declined: false },
        { person: ME, vote: "none", required: false, group: false, declined: false },
        { person: TEAM, vote: "waitingForAuthor", required: false, group: true, declined: false },
      ],
    },
    description: "Adds the login form and its validation.\n\nCloses #17.",
    threads: [
      thread(1, null, "active", [
        comment(1, 0, CAROL, "Looks good overall. Can we get a screenshot?", NOW - 5 * HOUR),
        comment(2, 1, BO, "Added one to the work item.", NOW - 4 * HOUR),
      ]),
      thread(2, { path: "src/login.ts", side: "new", line: 3 }, "active", [
        comment(1, 0, CAROL, "Should this trim the user name first?", NOW - 3 * HOUR),
      ]),
      thread(3, { path: "README.md", side: "old", line: 2 }, "fixed", [
        comment(1, 0, CAROL, "Keep the old setup steps somewhere?", NOW - 6 * HOUR),
        comment(2, 1, BO, "Moved them to the wiki.", NOW - 5 * HOUR),
      ]),
    ],
    fetched: true,
  };
  return [
    login,
    { summary: summary(41, "Bump dependencies", ME, "active", true, "chore/deps"), description: "", threads: [], fetched: false },
    { summary: summary(38, "Fix the crash on start", BO, "completed", false, "fix/crash"), description: "", threads: [], fetched: true },
  ];
}

function hunks(path: string): Hunk[] {
  const line = (kind: "context" | "added" | "removed", text: string) => ({ kind, text, noFinalNewline: false });
  if (path === "src/form.css") {
    const added = [".login {", "  display: grid;", "  gap: 8px;", "  max-width: 320px;", "}", ""].map((text) => line("added", text));
    return [{ oldStart: 0, oldLines: 0, newStart: 1, newLines: added.length, lines: added }];
  }
  if (path === "README.md") {
    return [
      {
        oldStart: 1,
        oldLines: 3,
        newStart: 1,
        newLines: 3,
        lines: [line("context", "# Mock app"), line("removed", "Run setup.sh first."), line("added", "See the wiki to set up."), line("context", "")],
      },
    ];
  }
  return [
    {
      oldStart: 1,
      oldLines: 4,
      newStart: 1,
      newLines: 8,
      lines: [
        line("context", "export function login(form: HTMLFormElement): void {"),
        line("removed", "  submit(form.user.value);"),
        line("added", "  const user = form.user.value;"),
        line("added", "  if (!user) {"),
        line("added", "    showError(form, \"Enter a user name\");"),
        line("added", "    return;"),
        line("added", "  }"),
        line("context", "}"),
        line("context", ""),
      ],
    },
  ];
}

/** Handles the review commands for the repository at `root`; `undefined` for any other command. */
export function mockReview(base: Oid, head: Oid): (command: string, args: Record<string, unknown>, root: string) => unknown {
  const prs = initial();
  let nextThread = 10;
  const find = (number: unknown) => prs.find((pr) => pr.summary.number === Number(number));
  const slowly = <T>(value: T) => new Promise<T>((resolve) => setTimeout(() => resolve(value), 300));
  const refuse = (error: ReviewError) => new Promise((_, reject) => setTimeout(() => reject(error), 300));

  function details(pr: MockPr): PullRequest {
    return {
      summary: pr.summary,
      description: pr.description,
      webUrl: `https://dev.azure.com/contoso/Web/_git/site/pullrequest/${pr.summary.number}`,
      me: ME,
      mergeStatus: "succeeded",
      iteration: 2,
      base,
      head,
      haveCommits: pr.fetched,
      threads: pr.threads,
    };
  }

  function act(pr: MockPr, action: ReviewAction): void {
    switch (action.kind) {
      case "vote": {
        const others = pr.summary.reviewers.filter((reviewer) => reviewer.person.id !== ME.id);
        const vote: Vote = action.vote;
        pr.summary = { ...pr.summary, reviewers: [...others, { person: ME, vote, required: false, group: false, declined: false }] };
        return;
      }
      case "comment":
        pr.threads = [...pr.threads, thread(nextThread++, action.anchor, "active", [comment(1, 0, ME, action.text, NOW)])];
        return;
      case "reply":
        pr.threads = pr.threads.map((t) =>
          t.id === action.thread ? { ...t, comments: [...t.comments, comment(t.comments.length + 1, action.parent, ME, action.text, NOW)] } : t,
        );
        return;
      case "setStatus":
        pr.threads = pr.threads.map((t) => (t.id === action.thread ? { ...t, status: action.status } : t));
    }
  }

  return (command, args, root) => {
    const azure = !root.endsWith("third-demo");
    switch (command) {
      case "review_host": {
        const host: ReviewHost | null = azure ? { remote: "origin", organization: "contoso", project: "Web", repository: "site" } : null;
        return host;
      }
      case "pull_requests": {
        const query = args.query as PullRequestQuery;
        const shown = prs.filter((pr) => {
          if (query === "completed" || query === "abandoned") return pr.summary.status === query;
          if (pr.summary.status !== "active") return false;
          if (query === "mine") return pr.summary.author.id === ME.id;
          if (query === "reviewing") return pr.summary.reviewers.some((reviewer) => reviewer.person.id === ME.id);
          return true;
        });
        return slowly(shown.map((pr) => pr.summary));
      }
      case "pull_request": {
        const pr = find(args.number);
        return pr ? slowly(details(pr)) : refuse({ kind: "notFound", message: "contoso: TF401180: The requested pull request was not found." });
      }
      case "review": {
        const pr = find(args.number);
        const action = args.action as ReviewAction;
        if (!pr) return refuse({ kind: "notFound", message: "contoso: That isn't there any more." });
        if ((action.kind === "comment" || action.kind === "reply") && action.text.includes("fail")) {
          return refuse({ kind: "forbidden", message: "contoso: You aren't allowed to comment on this pull request (mock)." });
        }
        act(pr, action);
        return slowly(null);
      }
      case "run_operation": {
        const op = args.operation as Operation;
        if (op.kind === "fetchPullRequest") {
          const pr = find(op.number);
          if (pr) pr.fetched = true;
        }
        return undefined;
      }
      case "changes": {
        const from = args.from as { kind: string; id?: string } | null;
        const to = args.to as { kind: string; id?: string };
        return from?.id === base && to.id === head ? FILES : undefined;
      }
      case "file_diff": {
        const diff: FileDiff = { kind: "text", hunks: hunks(String(args.path)) };
        return diff;
      }
      default:
        return undefined;
    }
  };
}
