//! Azure DevOps' REST API as it appears on the wire, and its translation into FerGit's model.
//!
//! Pure: no requests are made here. Fields are read leniently (anything missing takes a default),
//! since the service adds fields freely and Azure DevOps Server versions lag behind it.

use serde::Deserialize;
use serde_json::{Value, json};

use super::model::{
    Anchor, Comment, LineSide, MergeStatus, Person, PullRequestStatus, PullRequestSummary, Reviewer, Thread,
    ThreadStatus, Vote,
};
use crate::types::Oid;

/// The API version requested: the newest Azure DevOps Server 2020 supports, which has everything
/// FerGit uses.
pub(super) const API_VERSION: &str = "6.0";

/// `{ "value": [...] }`, how lists arrive.
#[derive(Deserialize)]
pub(super) struct List<T> {
    #[serde(default = "Vec::new")]
    pub value: Vec<T>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Identity {
    pub id: String,
    pub display_name: String,
    pub unique_name: String,
    pub is_container: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ConnectionData {
    pub authenticated_user: ConnectionUser,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ConnectionUser {
    pub id: String,
    pub provider_display_name: String,
    pub custom_display_name: Option<String>,
    pub properties: Value,
}

impl ConnectionUser {
    pub fn person(&self) -> Person {
        // The account's email address is among its properties, as `{ "$type": …, "$value": … }`.
        let account = self.properties.pointer("/Account/$value").and_then(Value::as_str).unwrap_or_default();
        Person {
            id: self.id.clone(),
            name: self.custom_display_name.clone().unwrap_or_else(|| self.provider_display_name.clone()),
            unique_name: account.to_owned(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ReviewerVote {
    #[serde(flatten)]
    pub identity: Identity,
    pub vote: i32,
    pub is_required: bool,
    pub has_declined: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct CommitRef {
    pub commit_id: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct PullRequest {
    pub pull_request_id: u32,
    pub title: String,
    pub description: String,
    pub status: String,
    pub created_by: Identity,
    pub creation_date: String,
    pub source_ref_name: String,
    pub target_ref_name: String,
    pub merge_status: String,
    pub is_draft: bool,
    pub reviewers: Vec<ReviewerVote>,
    pub last_merge_source_commit: Option<CommitRef>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Iteration {
    pub id: u32,
    pub source_ref_commit: Option<CommitRef>,
    pub common_ref_commit: Option<CommitRef>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct Position {
    pub line: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ThreadContext {
    pub file_path: Option<String>,
    pub left_file_start: Option<Position>,
    pub right_file_start: Option<Position>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ThreadComment {
    pub id: u32,
    pub parent_comment_id: u32,
    pub author: Identity,
    pub content: String,
    pub published_date: String,
    pub last_content_updated_date: String,
    pub comment_type: String,
    pub is_deleted: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct CommentThread {
    pub id: u32,
    pub status: Option<String>,
    pub thread_context: Option<ThreadContext>,
    pub comments: Vec<ThreadComment>,
    pub last_updated_date: String,
    pub is_deleted: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct IterationChanges {
    pub change_entries: Vec<ChangeEntry>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ChangeEntry {
    pub change_tracking_id: u32,
    pub item: ChangeItem,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub(super) struct ChangeItem {
    pub path: String,
}

/// The `message` of an error response, if it is one Azure DevOps wrote.
pub(super) fn error_message(body: &str) -> Option<String> {
    let value: Value = serde_json::from_str(body).ok()?;
    let message = value.get("message")?.as_str()?.trim();
    (!message.is_empty()).then(|| message.to_owned())
}

pub(super) fn person(identity: &Identity) -> Person {
    Person { id: identity.id.clone(), name: identity.display_name.clone(), unique_name: identity.unique_name.clone() }
}

pub(super) fn summary(pr: &PullRequest) -> PullRequestSummary {
    let status = match pr.status.as_str() {
        "completed" => PullRequestStatus::Completed,
        "abandoned" => PullRequestStatus::Abandoned,
        _ => PullRequestStatus::Active,
    };
    let mut reviewers: Vec<Reviewer> = pr
        .reviewers
        .iter()
        .map(|reviewer| Reviewer {
            person: person(&reviewer.identity),
            vote: vote(reviewer.vote),
            required: reviewer.is_required,
            group: reviewer.identity.is_container,
            declined: reviewer.has_declined,
        })
        .collect();
    // Required reviewers first, then people before groups, each by name: the order the web shows.
    reviewers.sort_by(|a, b| (!a.required, a.group, &a.person.name).cmp(&(!b.required, b.group, &b.person.name)));
    PullRequestSummary {
        number: pr.pull_request_id,
        title: pr.title.clone(),
        author: person(&pr.created_by),
        created_at: epoch_seconds(&pr.creation_date),
        status,
        draft: pr.is_draft,
        source_branch: branch_name(&pr.source_ref_name),
        target_branch: branch_name(&pr.target_ref_name),
        reviewers,
    }
}

pub(super) fn merge_status(pr: &PullRequest) -> MergeStatus {
    match pr.merge_status.as_str() {
        "succeeded" => MergeStatus::Succeeded,
        "conflicts" => MergeStatus::Conflicts,
        "queued" => MergeStatus::Pending,
        "failure" | "rejectedByPolicy" => MergeStatus::Failed,
        _ => MergeStatus::Unknown,
    }
}

fn branch_name(full_name: &str) -> String {
    full_name.strip_prefix("refs/heads/").unwrap_or(full_name).to_owned()
}

pub(super) fn commit_id(commit: Option<&CommitRef>) -> Option<Oid> {
    commit?.commit_id.parse().ok()
}

pub(super) fn vote(value: i32) -> Vote {
    match value {
        10.. => Vote::Approved,
        5..=9 => Vote::ApprovedWithSuggestions,
        -9..=-1 => Vote::WaitingForAuthor,
        ..=-10 => Vote::Rejected,
        _ => Vote::None,
    }
}

pub(super) fn vote_value(vote: Vote) -> i32 {
    match vote {
        Vote::Approved => 10,
        Vote::ApprovedWithSuggestions => 5,
        Vote::None => 0,
        Vote::WaitingForAuthor => -5,
        Vote::Rejected => -10,
    }
}

fn thread_status(status: &str) -> ThreadStatus {
    match status {
        "active" => ThreadStatus::Active,
        "pending" => ThreadStatus::Pending,
        "fixed" => ThreadStatus::Fixed,
        "wontFix" => ThreadStatus::WontFix,
        "closed" => ThreadStatus::Closed,
        "byDesign" => ThreadStatus::ByDesign,
        _ => ThreadStatus::Unknown,
    }
}

pub(super) fn thread_status_value(status: ThreadStatus) -> i32 {
    match status {
        ThreadStatus::Unknown => 0,
        ThreadStatus::Active => 1,
        ThreadStatus::Fixed => 2,
        ThreadStatus::WontFix => 3,
        ThreadStatus::Closed => 4,
        ThreadStatus::ByDesign => 5,
        ThreadStatus::Pending => 6,
    }
}

/// The discussion threads among `threads`, in order: deleted ones, and the host's own status
/// messages (votes cast, pushes, merge attempts), are left out, as are deleted comments.
pub(super) fn threads(threads: &[CommentThread]) -> Vec<Thread> {
    threads
        .iter()
        .filter(|thread| !thread.is_deleted)
        .filter_map(|thread| {
            let comments: Vec<Comment> = thread
                .comments
                .iter()
                .filter(|comment| !comment.is_deleted && comment.comment_type != "system")
                .map(|comment| Comment {
                    id: comment.id,
                    parent: comment.parent_comment_id,
                    author: person(&comment.author),
                    text: comment.content.clone(),
                    published_at: epoch_seconds(&comment.published_date),
                    edited: epoch_seconds(&comment.last_content_updated_date) > epoch_seconds(&comment.published_date),
                })
                .collect();
            if comments.is_empty() {
                return None;
            }
            Some(Thread {
                id: thread.id,
                status: thread.status.as_deref().map_or(ThreadStatus::Unknown, thread_status),
                anchor: thread.thread_context.as_ref().and_then(anchor),
                comments,
                updated_at: epoch_seconds(&thread.last_updated_date),
            })
        })
        .collect()
}

fn anchor(context: &ThreadContext) -> Option<Anchor> {
    let path = context.file_path.as_deref()?.trim_start_matches('/').to_owned();
    if path.is_empty() {
        return None;
    }
    let (side, line) = match (&context.right_file_start, &context.left_file_start) {
        (Some(right), _) => (LineSide::New, right.line),
        (None, Some(left)) => (LineSide::Old, left.line),
        (None, None) => (LineSide::New, 0),
    };
    Some(Anchor { path, side, line })
}

/// Azure DevOps' form of a repository path: `/`-rooted.
pub(super) fn host_path(path: &str) -> String {
    format!("/{}", path.trim_start_matches('/'))
}

/// The body that starts a thread with `text`, attached at `anchor` in iteration `iteration`.
/// `change` is the file's change tracking id in that iteration, which lets the host follow the
/// thread through later pushes; without one the thread stays where it was put.
pub(super) fn new_thread(text: &str, anchor: Option<&Anchor>, iteration: u32, change: Option<u32>) -> Value {
    let mut body = json!({
        "comments": [{ "parentCommentId": 0, "content": text, "commentType": 1 }],
        "status": thread_status_value(ThreadStatus::Active),
    });
    let Some(anchor) = anchor else {
        return body;
    };
    let mut context = json!({ "filePath": host_path(&anchor.path) });
    if anchor.line > 0 {
        let position = json!({ "line": anchor.line, "offset": 1 });
        let (start, end) = match anchor.side {
            LineSide::New => ("rightFileStart", "rightFileEnd"),
            LineSide::Old => ("leftFileStart", "leftFileEnd"),
        };
        context[start] = position.clone();
        context[end] = position;
    }
    body["threadContext"] = context;
    if let Some(change) = change {
        body["pullRequestThreadContext"] = json!({
            "changeTrackingId": change,
            "iterationContext": { "firstComparingIteration": iteration, "secondComparingIteration": iteration },
        });
    }
    body
}

/// Seconds since the Unix epoch of an ISO 8601 time as Azure DevOps writes them
/// (`2016-11-01T16:30:32.74Z`, perhaps with an offset instead of `Z`); 0 if it isn't one.
pub(super) fn epoch_seconds(text: &str) -> i64 {
    parse_time(text).unwrap_or(0)
}

fn parse_time(text: &str) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let digits = text.get(range)?;
        digits.bytes().all(|b| b.is_ascii_digit()).then(|| digits.parse().ok())?
    };
    let separators = [(4, b'-'), (7, b'-'), (10, b'T'), (13, b':'), (16, b':')];
    if separators.iter().any(|&(at, byte)| !text.as_bytes().get(at).is_some_and(|b| b.eq_ignore_ascii_case(&byte))) {
        return None;
    }
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let mut rest = text.get(19..)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        rest = fraction.trim_start_matches(|c: char| c.is_ascii_digit());
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let digits = rest[1..].replace(':', "");
            if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            sign * (digits[..2].parse::<i64>().ok()? * 3600 + digits[2..].parse::<i64>().ok()? * 60)
        }
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_azure_devops_times() {
        assert_eq!(epoch_seconds("1970-01-01T00:00:00Z"), 0);
        assert_eq!(epoch_seconds("2016-11-01T16:30:32.74Z"), 1_478_017_832);
        assert_eq!(epoch_seconds("2016-11-01T16:30:35Z"), 1_478_017_835);
        assert_eq!(epoch_seconds("2024-02-29T12:00:00.1234567Z"), 1_709_208_000);
        assert_eq!(epoch_seconds("2024-02-29T14:00:00+02:00"), 1_709_208_000);
        assert_eq!(epoch_seconds("2024-02-29T07:30:00-0430"), 1_709_208_000);
        for bad in ["", "yesterday", "2024-13-01T00:00:00Z", "2024-02-29 12:00:00Z", "2024-02-29T12:00:00+2"] {
            assert_eq!(epoch_seconds(bad), 0, "{bad}");
        }
    }

    #[test]
    fn votes_round_trip_and_odd_values_round_to_the_nearest_meaning() {
        for vote in [Vote::Approved, Vote::ApprovedWithSuggestions, Vote::None, Vote::WaitingForAuthor, Vote::Rejected] {
            assert_eq!(super::vote(vote_value(vote)), vote);
        }
        assert_eq!(super::vote(7), Vote::ApprovedWithSuggestions);
        assert_eq!(super::vote(-1), Vote::WaitingForAuthor);
    }

    #[test]
    fn keeps_discussions_and_leaves_out_status_messages_and_deleted_comments() {
        let sample = r#"{ "value": [
            { "id": 1, "comments": [{ "id": 1, "content": "Normal Paulk voted 10", "commentType": "system" }] },
            { "id": 2, "status": "active", "lastUpdatedDate": "2016-11-01T16:30:52.48Z",
              "threadContext": { "filePath": "/src/new_feature.cpp", "rightFileStart": { "line": 5, "offset": 1 } },
              "comments": [
                { "id": 1, "parentCommentId": 0, "content": "Explain this value?", "commentType": "text",
                  "author": { "id": "u1", "displayName": "Normal Paulk", "uniqueName": "np@example.com" },
                  "publishedDate": "2016-11-01T16:30:50Z", "lastContentUpdatedDate": "2016-11-01T16:30:50Z" },
                { "id": 2, "parentCommentId": 1, "isDeleted": true, "commentType": "text" },
                { "id": 3, "parentCommentId": 1, "content": "Done", "commentType": "text",
                  "publishedDate": "2016-11-01T16:31:00Z", "lastContentUpdatedDate": "2016-11-01T16:32:00Z" }
              ] },
            { "id": 3, "status": "fixed", "threadContext": { "filePath": "/old.txt", "leftFileStart": { "line": 2 } },
              "comments": [{ "id": 1, "content": "Why remove this?", "commentType": "text" }] },
            { "id": 4, "status": "byDesign", "threadContext": { "filePath": "/README.md" },
              "comments": [{ "id": 1, "content": "About the whole file", "commentType": "text" }] },
            { "id": 5, "isDeleted": true, "comments": [{ "id": 1, "content": "gone", "commentType": "text" }] }
        ] }"#;
        let list: List<CommentThread> = serde_json::from_str(sample).unwrap();
        let threads = threads(&list.value);

        assert_eq!(threads.iter().map(|t| t.id).collect::<Vec<_>>(), [2, 3, 4]);
        let first = &threads[0];
        assert_eq!(first.status, ThreadStatus::Active);
        assert_eq!(first.anchor, Some(Anchor { path: "src/new_feature.cpp".into(), side: LineSide::New, line: 5 }));
        assert_eq!(first.comments.iter().map(|c| (c.id, c.parent, c.edited)).collect::<Vec<_>>(), [(1, 0, false), (3, 1, true)]);
        assert_eq!(first.comments[0].author.name, "Normal Paulk");
        assert_eq!(threads[1].anchor, Some(Anchor { path: "old.txt".into(), side: LineSide::Old, line: 2 }));
        assert_eq!(threads[1].status, ThreadStatus::Fixed);
        assert_eq!(threads[2].anchor, Some(Anchor { path: "README.md".into(), side: LineSide::New, line: 0 }));
    }

    #[test]
    fn summarizes_a_pull_request() {
        let sample = r#"{
            "pullRequestId": 22, "title": "Add a feature", "status": "active", "isDraft": true,
            "createdBy": { "id": "u1", "displayName": "Normal Paulk", "uniqueName": "np@example.com" },
            "creationDate": "2016-11-01T16:30:31.6655471Z",
            "sourceRefName": "refs/heads/npaulk/my_work", "targetRefName": "refs/heads/new_feature",
            "mergeStatus": "conflicts",
            "reviewers": [
                { "id": "t1", "displayName": "Web Team", "isContainer": true, "vote": 0 },
                { "id": "u3", "displayName": "Zed", "vote": -5 },
                { "id": "u2", "displayName": "Johnnie McLeod", "vote": 10, "isRequired": true }
            ],
            "lastMergeSourceCommit": { "commitId": "b60280bc6e62e2f880f1b63c1e24987664d3bda3" },
            "someFieldFromTheFuture": [1, 2, 3]
        }"#;
        let pr: PullRequest = serde_json::from_str(sample).unwrap();
        let summary = summary(&pr);

        assert_eq!((summary.number, summary.title.as_str(), summary.draft), (22, "Add a feature", true));
        assert_eq!((summary.source_branch.as_str(), summary.target_branch.as_str()), ("npaulk/my_work", "new_feature"));
        assert_eq!(summary.status, PullRequestStatus::Active);
        assert_eq!(summary.created_at, 1_478_017_831);
        let names: Vec<(&str, Vote)> = summary.reviewers.iter().map(|r| (r.person.name.as_str(), r.vote)).collect();
        assert_eq!(names, [("Johnnie McLeod", Vote::Approved), ("Zed", Vote::WaitingForAuthor), ("Web Team", Vote::None)]);
        assert!(summary.reviewers[2].group);
        assert_eq!(merge_status(&pr), MergeStatus::Conflicts);
        assert_eq!(commit_id(pr.last_merge_source_commit.as_ref()).unwrap().to_string(), "b60280bc6e62e2f880f1b63c1e24987664d3bda3");
    }

    #[test]
    fn a_new_thread_is_attached_to_a_line_of_one_side() {
        let anchor = Anchor { path: "src/a.rs".into(), side: LineSide::Old, line: 12 };
        assert_eq!(
            new_thread("Why?", Some(&anchor), 3, Some(7)),
            json!({
                "comments": [{ "parentCommentId": 0, "content": "Why?", "commentType": 1 }],
                "status": 1,
                "threadContext": {
                    "filePath": "/src/a.rs",
                    "leftFileStart": { "line": 12, "offset": 1 },
                    "leftFileEnd": { "line": 12, "offset": 1 }
                },
                "pullRequestThreadContext": {
                    "changeTrackingId": 7,
                    "iterationContext": { "firstComparingIteration": 3, "secondComparingIteration": 3 }
                }
            })
        );
        let general = new_thread("Looks good", None, 3, None);
        assert!(general.get("threadContext").is_none() && general.get("pullRequestThreadContext").is_none());
        let whole_file = new_thread("Hm", Some(&Anchor { path: "a.rs".into(), side: LineSide::New, line: 0 }), 3, None);
        assert_eq!(whole_file["threadContext"], json!({ "filePath": "/a.rs" }));
    }

    #[test]
    fn reads_the_message_of_an_error() {
        let body = r#"{"$id":"1","innerException":null,"message":"TF401180: The requested pull request was not found.","errorCode":0}"#;
        assert_eq!(error_message(body).as_deref(), Some("TF401180: The requested pull request was not found."));
        assert_eq!(error_message("<html>Sign in</html>"), None);
    }
}
