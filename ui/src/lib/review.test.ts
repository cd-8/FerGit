import { beforeEach, describe, expect, it } from "vitest";
import type { FileChange, Operation, PullRequest, PullRequestSummary, Reviewer, Thread, Vote } from "./bindings";
import { DiffModel } from "./diff";
import {
  anchorText,
  asReviewError,
  generalThreads,
  isResolved,
  lineKey,
  myVote,
  retryLabel,
  threadCounts,
  threadsByLine,
  threadsOn,
  verdicts,
} from "./review";
import { Review } from "./review.svelte";

const person = (id: string, name = id) => ({ id, name, uniqueName: `${id}@example.com` });
const reviewer = (id: string, vote: Vote): Reviewer => ({ person: person(id), vote, required: false, group: false, declined: false });

function thread(id: number, anchor: Thread["anchor"], status: Thread["status"] = "active"): Thread {
  return { id, status, anchor, comments: [{ id: 1, parent: 0, author: person("a"), text: "Hi", publishedAt: 0, edited: false }], updatedAt: 0 };
}

const at = (path: string, line: number, side: "old" | "new" = "new") => ({ path, line, side });

function summary(number: number, reviewers: Reviewer[] = []): PullRequestSummary {
  return {
    number,
    title: `PR ${number}`,
    author: person("bo"),
    createdAt: 0,
    status: "active",
    draft: false,
    sourceBranch: "feature",
    targetBranch: "main",
    reviewers,
  };
}

function pullRequest(number: number, changes: Partial<PullRequest> = {}): PullRequest {
  return {
    summary: summary(number, [reviewer("me", "waitingForAuthor")]),
    description: "",
    webUrl: "",
    me: person("me"),
    mergeStatus: "succeeded",
    iteration: 2,
    base: "b".repeat(40),
    head: "c".repeat(40),
    haveCommits: true,
    threads: [],
    ...changes,
  };
}

describe("review helpers", () => {
  const threads = [
    thread(1, null),
    thread(2, at("b.rs", 9)),
    thread(3, at("a.rs", 4), "fixed"),
    thread(4, at("a.rs", 0)),
    thread(5, at("a.rs", 4, "old")),
    thread(6, at("a.rs", 4)),
  ];

  it("groups threads by file, whole-file threads first, then by line, old side first", () => {
    expect(generalThreads(threads).map((t) => t.id)).toEqual([1]);
    expect(threadsOn(threads, "a.rs").map((t) => t.id)).toEqual([4, 5, 3, 6]);
    expect(threadCounts(threads).get("a.rs")).toEqual({ total: 4, open: 3 });
    expect(threadCounts(threads).get("b.rs")).toEqual({ total: 1, open: 1 });
  });

  it("finds threads by the line they are on", () => {
    const byLine = threadsByLine(threads, "a.rs");
    expect(byLine.get(lineKey("new", 4))?.map((t) => t.id)).toEqual([3, 6]);
    expect(byLine.get(lineKey("old", 4))?.map((t) => t.id)).toEqual([5]);
    expect([...byLine.keys()]).not.toContain(lineKey("new", 0));
  });

  it("says where a thread is and whether it is resolved", () => {
    expect(anchorText(at("a.rs", 0))).toBe("the whole file");
    expect(anchorText(at("a.rs", 3, "old"))).toBe("line 3 before the changes");
    expect(anchorText(at("a.rs", 3))).toBe("line 3");
    expect(["fixed", "wontFix", "closed", "byDesign"].every((s) => isResolved(s as Thread["status"]))).toBe(true);
    expect(isResolved("active") || isResolved("pending")).toBe(false);
  });

  it("counts verdicts and finds the signed-in user's vote", () => {
    const reviewers = [reviewer("a", "approved"), reviewer("b", "approvedWithSuggestions"), reviewer("c", "rejected"), reviewer("d", "none")];
    expect(verdicts(reviewers)).toEqual({ approved: 2, waiting: 0, rejected: 1 });
    expect(myVote(pullRequest(1))).toBe("waitingForAuthor");
    expect(myVote(pullRequest(1, { me: person("stranger") }))).toBe("none");
  });

  it("offers to try again only where that can help", () => {
    expect(retryLabel({ kind: "signIn", message: "" })).toBe("Sign in again");
    expect(retryLabel({ kind: "forbidden", message: "" })).toBeNull();
    expect(asReviewError(new Error("boom"))).toEqual({ kind: "server", message: "boom" });
    expect(asReviewError({ kind: "notFound", message: "gone" })).toEqual({ kind: "notFound", message: "gone" });
  });

  it("finds the display row of a line of either side", () => {
    const model = new DiffModel([
      {
        oldStart: 10,
        oldLines: 2,
        newStart: 10,
        newLines: 2,
        lines: [
          { kind: "context", text: "a", noFinalNewline: false },
          { kind: "removed", text: "b", noFinalNewline: false },
          { kind: "added", text: "c", noFinalNewline: false },
        ],
      },
    ]);
    expect([model.rowOf("old", 10), model.rowOf("new", 10)]).toEqual([1, 1]);
    expect([model.rowOf("old", 11), model.rowOf("new", 11)]).toEqual([2, 3]);
    expect([model.rowOf("new", 3), model.rowOf("new", 0)]).toEqual([null, null]);
  });
});

interface Call {
  command: string;
  args: unknown[];
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
}

describe("Review", () => {
  let calls: Call[];
  let review: Review;
  let ran: Operation[];

  const defer =
    (command: string) =>
    (...args: unknown[]) =>
      new Promise<never>((resolve, reject) => calls.push({ command, args, resolve: resolve as (value: unknown) => void, reject }));

  function take(command: string): Call {
    const index = calls.findIndex((call) => call.command === command);
    if (index < 0) throw new Error(`no pending ${command} call; pending: ${calls.map((c) => c.command).join(", ")}`);
    return calls.splice(index, 1)[0];
  }

  const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

  async function answer(command: string, value: unknown): Promise<Call> {
    const call = take(command);
    call.resolve(value);
    await settle();
    return call;
  }

  beforeEach(async () => {
    calls = [];
    ran = [];
    const client = {
      reviewHost: defer("reviewHost"),
      pullRequests: defer("pullRequests"),
      pullRequest: defer("pullRequest"),
      review: defer("review"),
      changes: defer("changes"),
      fileDiff: defer("fileDiff"),
    };
    const operations = {
      run: async (op: Operation) => {
        ran.push(op);
        return { kind: "done", info: {} } as never;
      },
    };
    review = new Review(client, operations);
    void review.findHost();
    await answer("reviewHost", { remote: "origin", organization: "contoso", project: "Web", repository: "site" });
  });

  it("contacts the host only once opened, and keeps the list shown while reloading", async () => {
    expect(calls).toEqual([]);
    review.toggle();
    expect(review.open).toBe(true);
    expect(review.list.loading).toBe(true);
    await answer("pullRequests", [summary(1)]);
    expect(review.list.value?.map((pr) => pr.number)).toEqual([1]);

    review.setQuery("completed");
    expect(take("pullRequests").args).toEqual(["completed"]);
    expect(review.list).toMatchObject({ loading: true, value: [{ number: 1 }] });
  });

  it("can't be opened without an Azure DevOps remote", async () => {
    void review.findHost();
    await answer("reviewHost", null);
    review.toggle();
    expect(review.open).toBe(false);
    expect(calls).toEqual([]);
  });

  it("shows a failure in place, and drops answers about a pull request no longer selected", async () => {
    review.toggle();
    take("pullRequests").reject({ kind: "signIn", message: "No credentials" });
    await settle();
    expect(review.list.error?.kind).toBe("signIn");

    review.select(1);
    review.select(2);
    take("pullRequest").resolve(pullRequest(1));
    await settle();
    expect(review.pullRequest.value).toBeUndefined();
    await answer("pullRequest", pullRequest(2, { haveCommits: false }));
    expect(review.pullRequest.value?.summary.number).toBe(2);
    expect(calls.filter((c) => c.command === "changes")).toEqual([]);
  });

  it("reads the files between the base and the head, again only when they move", async () => {
    review.select(3);
    await answer("pullRequest", pullRequest(3));
    const files: FileChange[] = [{ path: "a.rs", oldPath: null, status: "modified", additions: 1, deletions: 0 }];
    const changes = await answer("changes", files);
    expect(changes.args).toEqual([{ kind: "commit", id: "b".repeat(40) }, { kind: "commit", id: "c".repeat(40) }]);
    expect(review.files).toEqual(files);

    void review.reload();
    await answer("pullRequest", pullRequest(3));
    expect(calls).toEqual([]);
    expect(review.files).toEqual(files);
  });

  it("acts once per key at a time, then reads the pull request again", async () => {
    review.select(4);
    await answer("pullRequest", pullRequest(4));
    take("changes");

    const voting = review.act("vote", { kind: "vote", vote: "approved" });
    expect(await review.act("vote", { kind: "vote", vote: "approved" })).toBe(false);
    expect(review.acting.has("vote")).toBe(true);
    const call = await answer("review", undefined);
    expect(call.args.slice(1)).toEqual([4, { kind: "vote", vote: "approved" }]);
    expect(await voting).toBe(true);
    expect(review.acting.size).toBe(0);
    take("pullRequest");
    take("pullRequests");

    const replying = review.act("reply:9", { kind: "reply", thread: 9, parent: 1, text: "Ok" });
    take("review").reject({ kind: "forbidden", message: "Not allowed" });
    expect(await replying).toBe(false);
    expect(review.actionError?.message).toBe("Not allowed");
    expect(calls).toEqual([]);
  });

  it("fetches the pull request's commits from the host's remote, then reads it again", async () => {
    review.select(5);
    await answer("pullRequest", pullRequest(5, { haveCommits: false }));
    const fetching = review.fetchCommits();
    await settle();
    expect(ran).toEqual([{ kind: "fetchPullRequest", remote: "origin", number: 5, branch: "feature", head: "c".repeat(40) }]);
    await answer("pullRequest", pullRequest(5));
    await answer("changes", []);
    await fetching;
    expect(review.fetching).toBe(false);
    expect(review.files).toEqual([]);
  });
});
