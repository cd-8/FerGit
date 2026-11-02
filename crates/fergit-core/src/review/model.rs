//! What pull request review shows and does, in FerGit's terms. Sent to and from the UI; nothing here
//! is specific to how Azure DevOps words it on the wire.

use serde::{Deserialize, Serialize};

use crate::types::Oid;

/// Where a repository's pull requests live: the Azure DevOps repository one of its remotes names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ReviewHost {
    /// The remote naming it, e.g. `origin`; pull requests' commits are fetched from it.
    pub remote: String,
    /// The organization (`contoso`), or a server's collection (`tfs.contoso.local/tfs/Main`).
    pub organization: String,
    pub project: String,
    pub repository: String,
}

/// Which pull requests to list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PullRequestQuery {
    /// Every active one.
    Active,
    /// Active ones the signed-in user created.
    Mine,
    /// Active ones the signed-in user is a reviewer of (directly or through a group).
    Reviewing,
    Completed,
    Abandoned,
}

/// A user or group, as the host identifies them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Person {
    /// The host's id for them; stable, unlike names.
    pub id: String,
    pub name: String,
    /// Usually an email address; for groups, `[Project]\Team`.
    pub unique_name: String,
}

/// A reviewer's verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Vote {
    Approved,
    ApprovedWithSuggestions,
    /// No vote (yet), or a vote reset.
    None,
    WaitingForAuthor,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Reviewer {
    pub person: Person,
    pub vote: Vote,
    /// A branch policy requires their vote.
    pub required: bool,
    /// A group or team, whose vote rolls up its members'.
    pub group: bool,
    /// They declined to review.
    pub declined: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PullRequestStatus {
    Active,
    Completed,
    Abandoned,
}

/// A pull request, as lists show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSummary {
    pub number: u32,
    pub title: String,
    pub author: Person,
    /// Seconds since the Unix epoch.
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub created_at: i64,
    pub status: PullRequestStatus,
    pub draft: bool,
    /// Branch names without `refs/heads/`: what is merged, and where to.
    pub source_branch: String,
    pub target_branch: String,
    pub reviewers: Vec<Reviewer>,
}

/// Whether the host could merge the pull request when it last tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum MergeStatus {
    Succeeded,
    Conflicts,
    /// A merge is being attempted.
    Pending,
    /// It failed, or a policy rejected it.
    Failed,
    Unknown,
}

/// Everything shown about one pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub summary: PullRequestSummary,
    pub description: String,
    /// Its page in the host's web interface.
    pub web_url: String,
    /// The signed-in user: whose vote the vote buttons set.
    pub me: Person,
    pub merge_status: MergeStatus,
    /// Its latest iteration (each push to the source branch makes one), which `head` and the
    /// threads' lines refer to.
    pub iteration: u32,
    /// What its changes are measured from: the commit its source and target branches last had in
    /// common. `None` before the host has computed it.
    pub base: Option<Oid>,
    /// Its latest commit.
    pub head: Option<Oid>,
    /// Whether `base` and `head` are both in the local repository, so its changes can be shown;
    /// fetch them with the `fetchPullRequest` operation if not.
    pub have_commits: bool,
    /// Discussion threads, oldest first; status messages from the host are left out.
    pub threads: Vec<Thread>,
}

/// Where a thread is attached in a pull request's changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    /// The file, `/`-separated from the repository's root.
    pub path: String,
    /// The version of the file `line` counts in: before the changes (`old`) or after (`new`).
    pub side: LineSide,
    /// 1-based line number; 0 attaches the thread to the file as a whole.
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LineSide {
    Old,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ThreadStatus {
    Active,
    Pending,
    /// Resolved.
    Fixed,
    WontFix,
    Closed,
    ByDesign,
    Unknown,
}

/// A comment and its replies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Thread {
    pub id: u32,
    pub status: ThreadStatus,
    /// `None` for a comment on the pull request as a whole.
    pub anchor: Option<Anchor>,
    /// Oldest first; deleted comments are left out.
    pub comments: Vec<Comment>,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    /// Unique within its thread.
    pub id: u32,
    /// The comment this replies to; 0 for the thread's first comment.
    pub parent: u32,
    pub author: Person,
    /// Markdown, as the author wrote it. Shown as plain text.
    pub text: String,
    #[cfg_attr(feature = "specta", specta(type = specta_typescript::Number))]
    pub published_at: i64,
    /// Changed since it was published.
    pub edited: bool,
}

/// Something the signed-in user does to a pull request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ReviewAction {
    /// Set their vote; `none` resets it.
    Vote { vote: Vote },
    /// Start a thread: on the pull request as a whole, or at `anchor` in the changes of iteration
    /// `iteration` (the [`PullRequest::iteration`] shown), which its line counts in.
    Comment { text: String, anchor: Option<Anchor>, iteration: u32 },
    /// Answer comment `parent` of `thread`.
    Reply { thread: u32, parent: u32, text: String },
    /// Resolve a thread, or make it active again.
    SetStatus { thread: u32, status: ThreadStatus },
}

/// Why talking to the host failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct ReviewError {
    pub kind: ReviewErrorKind,
    /// What went wrong, for the user.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ReviewErrorKind {
    /// No remote names an Azure DevOps repository.
    NoHost,
    /// No credentials, or the host refused them; trying again asks for new ones.
    SignIn,
    /// Signed in, but not allowed to do this.
    Forbidden,
    NotFound,
    /// The host couldn't be reached.
    Unreachable,
    /// The host refused the request as it was.
    Refused,
    /// The host failed, or answered something FerGit doesn't understand.
    Server,
}

impl ReviewError {
    pub(crate) fn new(kind: ReviewErrorKind, message: impl Into<String>) -> ReviewError {
        ReviewError { kind, message: message.into() }
    }
}
