//! Pull request review against a fake Azure DevOps server on loopback, with credentials from git's
//! `store` helper: what is requested, how it signs in, and what comes back.

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use common::{Fixture, T0, commit, git};
use fergit_core::repo::Repo;
use fergit_core::review::{
    Anchor, LineSide, PullRequestQuery, ReviewAction, ReviewErrorKind, Reviews, ThreadStatus, Vote,
};
use fergit_core::OpId;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
struct Request {
    method: String,
    /// Path and query, e.g. `/Main/Web/_apis/...?api-version=6.0`.
    target: String,
    authorization: String,
    body: Value,
}

type Handler = dyn Fn(&Request) -> (u16, Value) + Send + Sync;

/// An HTTP server answering every request with `handler`, recording them all.
struct FakeAzure {
    port: u16,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl FakeAzure {
    fn start(handler: impl Fn(&Request) -> (u16, Value) + Send + Sync + 'static) -> FakeAzure {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let (seen, handler): (_, Arc<Handler>) = (Arc::clone(&requests), Arc::new(handler));
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let (method, target) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
                let (mut length, mut authorization) = (0, String::new());
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    let (name, value) = header.split_once(':').unwrap();
                    match name.to_ascii_lowercase().as_str() {
                        "content-length" => length = value.trim().parse().unwrap(),
                        "authorization" => authorization = value.trim().to_owned(),
                        _ => {}
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                let body = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
                let request = Request { method, target, authorization, body };
                let (status, answer) = handler(&request);
                seen.lock().unwrap().push(request);
                let answer = answer.to_string();
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        FakeAzure { port, requests }
    }

    fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// `METHOD target` of every request so far, without the API version.
    fn calls(&self) -> Vec<String> {
        self.requests()
            .iter()
            .map(|r| {
                let parts: Vec<&str> = r.target.split(['?', '&']).filter(|part| *part != "api-version=6.0").collect();
                format!("{} {}", r.method, parts.join(" "))
            })
            .collect()
    }
}

const API: &str = "/Main/Web/_apis/git/repositories/site";
const ME: &str = "6a1c7d2e-0000-4000-8000-00000000000a";

/// `Basic` credentials as git's helpers would send them.
fn basic(user: &str, password: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = format!("{user}:{password}").into_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    format!("Basic {out}")
}

fn connection_data() -> Value {
    json!({ "authenticatedUser": { "id": ME, "providerDisplayName": "Ada Reviewer",
        "properties": { "Account": { "$type": "System.String", "$value": "ada@example.com" } } } })
}

fn pull_request(number: u32) -> Value {
    json!({
        "pullRequestId": number, "title": "Add the login form", "status": "active", "description": "Adds it.",
        "createdBy": { "id": "u-author", "displayName": "Bo Author", "uniqueName": "bo@example.com" },
        "creationDate": "2026-10-30T09:00:00Z", "sourceRefName": "refs/heads/feature/login",
        "targetRefName": "refs/heads/main", "mergeStatus": "succeeded",
        "reviewers": [{ "id": ME, "displayName": "Ada Reviewer", "vote": 0 }]
    })
}

/// A repository whose `origin` is the fake server, with `password` stored for it by git's `store`
/// helper. Returns the repository and the store file.
fn repository(fx: &Fixture, server: &FakeAzure, password: &str) -> (PathBuf, PathBuf) {
    let path = fx.init("repo");
    git(&path, &["remote", "add", "origin", &format!("http://127.0.0.1:{}/Main/Web/_git/site", server.port)]);
    let store = fx.path("credentials");
    store_password(&store, server, password);
    let helper = format!("store --file={}", store.display().to_string().replace('\\', "/"));
    git(&path, &["config", "credential.helper", &helper]);
    (path, store)
}

fn store_password(store: &Path, server: &FakeAzure, password: &str) {
    std::fs::write(store, format!("http://ada:{password}@127.0.0.1:{}\n", server.port)).unwrap();
}

#[test]
fn finds_the_azure_devops_remote_without_contacting_it() {
    let fx = Fixture::new();
    let path = fx.init("repo");
    let repo = Repo::open(&path).unwrap();
    assert_eq!(Reviews::host(&repo), None);

    git(&path, &["remote", "add", "github", "https://github.com/o/r.git"]);
    git(&path, &["remote", "add", "azure", "git@ssh.dev.azure.com:v3/contoso/My%20Project/site"]);
    let host = Reviews::host(&repo).unwrap();
    assert_eq!(
        (host.remote.as_str(), host.organization.as_str(), host.project.as_str(), host.repository.as_str()),
        ("azure", "contoso", "My Project", "site")
    );

    let error = Reviews::new().pull_requests(&Repo::open(&fx.init("plain")).unwrap(), None, PullRequestQuery::Active);
    assert_eq!(error.unwrap_err().kind, ReviewErrorKind::NoHost);
}

#[test]
fn lists_pull_requests_signed_in_with_the_credential_git_would_use() {
    let fx = Fixture::new();
    let server = FakeAzure::start(|request| match request.target.split('?').next().unwrap() {
        "/Main/_apis/connectionData" => (200, connection_data()),
        _ => (200, json!({ "value": [pull_request(5)], "count": 1 })),
    });
    let (path, _) = repository(&fx, &server, "t0ken");
    let repo = Repo::open(&path).unwrap();
    let reviews = Reviews::new();

    let active = reviews.pull_requests(&repo, None, PullRequestQuery::Active).unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!((active[0].number, active[0].source_branch.as_str()), (5, "feature/login"));
    reviews.pull_requests(&repo, None, PullRequestQuery::Reviewing).unwrap();

    assert_eq!(
        server.calls(),
        [
            format!("GET {API}/pullrequests searchCriteria.status=active $top=100"),
            "GET /Main/_apis/connectionData".to_owned(),
            format!("GET {API}/pullrequests searchCriteria.status=active $top=100 searchCriteria.reviewerId={ME}"),
        ]
    );
    assert!(server.requests().iter().all(|r| r.authorization == basic("ada", "t0ken")));
}

#[test]
fn reads_a_pull_request_with_its_discussion_and_whether_its_commits_are_here() {
    let fx = Fixture::new();
    let path = fx.init("repo");
    let base = commit(&path, "a.txt", "1\n", "Base", T0);
    let head = commit(&path, "a.txt", "2\n", "Head", T0 + 60);
    std::fs::remove_dir_all(&path).unwrap();
    let (base_id, head_id) = (base.to_string(), head.to_string());
    let server = FakeAzure::start(move |request| match request.target.split('?').next().unwrap() {
        "/Main/_apis/connectionData" => (200, connection_data()),
        target if target.ends_with("/iterations") => (200, json!({ "value": [
            { "id": 1, "sourceRefCommit": { "commitId": "1".repeat(40) }, "commonRefCommit": { "commitId": base_id } },
            { "id": 2, "sourceRefCommit": { "commitId": head_id }, "commonRefCommit": { "commitId": base_id } }
        ] })),
        target if target.ends_with("/threads") => (200, json!({ "value": [
            { "id": 9, "status": "active", "comments": [{ "id": 1, "content": "Nice", "commentType": "text",
              "author": { "id": "u-author", "displayName": "Bo Author" } }],
              "threadContext": { "filePath": "/a.txt", "rightFileStart": { "line": 1, "offset": 1 } } },
            { "id": 10, "comments": [{ "id": 1, "content": "Ada voted 10", "commentType": "system" }] }
        ] })),
        _ => (200, pull_request(5)),
    });
    let (path, _) = repository(&fx, &server, "t0ken");
    let repo = Repo::open(&path).unwrap();
    let reviews = Reviews::new();

    let pr = reviews.pull_request(&repo, None, 5).unwrap();
    assert_eq!((pr.iteration, pr.base, pr.head), (2, Some(base), Some(head)));
    assert!(!pr.have_commits, "the commits haven't been fetched");
    assert_eq!(pr.me.name, "Ada Reviewer");
    assert_eq!(pr.me.unique_name, "ada@example.com");
    assert_eq!(pr.threads.len(), 1, "the vote message is left out");
    assert_eq!(pr.threads[0].anchor, Some(Anchor { path: "a.txt".into(), side: LineSide::New, line: 1 }));
    assert!(pr.web_url.ends_with("/Main/Web/_git/site/pullrequest/5"), "{}", pr.web_url);
    assert!(server.calls().contains(&format!("GET {API}/pullRequests/5/threads $iteration=2")));

    commit(&path, "a.txt", "1\n", "Base", T0);
    commit(&path, "a.txt", "2\n", "Head", T0 + 60);
    assert!(reviews.pull_request(&repo, None, 5).unwrap().have_commits);
    let signed_in = server.calls().iter().filter(|call| call.contains("connectionData")).count();
    assert_eq!(signed_in, 1, "who is signed in is asked once");
}

#[test]
fn votes_comments_replies_and_resolves_once_per_request() {
    let fx = Fixture::new();
    let server = FakeAzure::start(|request| match request.target.split('?').next().unwrap() {
        "/Main/_apis/connectionData" => (200, connection_data()),
        target if target.ends_with("/changes") => {
            (200, json!({ "changeEntries": [{ "changeTrackingId": 4, "item": { "path": "/src/a.rs" } }] }))
        }
        _ => (200, json!({})),
    });
    let (path, _) = repository(&fx, &server, "t0ken");
    let repo = Repo::open(&path).unwrap();
    let reviews = Reviews::new();
    let act = |id: &str, action: ReviewAction| reviews.act(&repo, None, OpId(id.into()), 5, &action);

    act("1", ReviewAction::Vote { vote: Vote::Approved }).unwrap();
    act("1", ReviewAction::Vote { vote: Vote::Approved }).unwrap();
    let anchor = Anchor { path: "src/a.rs".into(), side: LineSide::New, line: 3 };
    act("2", ReviewAction::Comment { text: "Why?".into(), anchor: Some(anchor), iteration: 2 }).unwrap();
    act("3", ReviewAction::Reply { thread: 9, parent: 1, text: "Because.".into() }).unwrap();
    act("4", ReviewAction::SetStatus { thread: 9, status: ThreadStatus::Fixed }).unwrap();
    let blank = act("5", ReviewAction::Reply { thread: 9, parent: 1, text: "  ".into() }).unwrap_err();
    assert_eq!(blank.kind, ReviewErrorKind::Refused);

    assert_eq!(
        server.calls(),
        [
            "GET /Main/_apis/connectionData".to_owned(),
            format!("PUT {API}/pullRequests/5/reviewers/{ME}"),
            format!("GET {API}/pullRequests/5/iterations/2/changes $top=2000"),
            format!("POST {API}/pullRequests/5/threads"),
            format!("POST {API}/pullRequests/5/threads/9/comments"),
            format!("PATCH {API}/pullRequests/5/threads/9"),
        ]
    );
    let bodies: Vec<Value> = server.requests().into_iter().map(|r| r.body).collect();
    assert_eq!(bodies[1], json!({ "vote": 10, "id": ME }));
    assert_eq!(bodies[3]["threadContext"]["rightFileStart"], json!({ "line": 3, "offset": 1 }));
    assert_eq!(bodies[3]["pullRequestThreadContext"]["changeTrackingId"], 4);
    assert_eq!(bodies[4], json!({ "content": "Because.", "parentCommentId": 1, "commentType": 1 }));
    assert_eq!(bodies[5], json!({ "status": 2 }));
}

#[test]
fn a_refused_credential_is_forgotten_and_an_expired_one_replaced() {
    let fx = Fixture::new();
    let accepted = Arc::new(Mutex::new(basic("ada", "good")));
    let current = Arc::clone(&accepted);
    let server = FakeAzure::start(move |request| match request.authorization == *current.lock().unwrap() {
        true => (200, json!({ "value": [] })),
        false => (401, json!({ "message": "TF400813: not authorized" })),
    });
    let (path, store) = repository(&fx, &server, "bad");
    let repo = Repo::open(&path).unwrap();
    let reviews = Reviews::new();
    let list = || reviews.pull_requests(&repo, None, PullRequestQuery::Active);

    assert_eq!(list().unwrap_err().kind, ReviewErrorKind::SignIn);
    assert!(!std::fs::read_to_string(&store).unwrap().contains("bad"), "the helper forgot it");

    store_password(&store, &server, "good");
    list().unwrap();
    list().unwrap();
    assert_eq!(server.requests().iter().filter(|r| r.authorization == basic("ada", "good")).count(), 2);

    // The token kept in memory expires; the helper has a new one by now.
    *accepted.lock().unwrap() = basic("ada", "newer");
    store_password(&store, &server, "newer");
    list().unwrap();
    let last: Vec<String> = server.requests().iter().rev().take(2).map(|r| r.authorization.clone()).collect();
    assert_eq!(last, [basic("ada", "newer"), basic("ada", "good")]);
}

#[test]
fn errors_say_what_the_host_said() {
    let fx = Fixture::new();
    let server = FakeAzure::start(|_| (404, json!({ "message": "TF401180: The requested pull request was not found." })));
    let (path, _) = repository(&fx, &server, "t0ken");
    let error = Reviews::new().pull_request(&Repo::open(&path).unwrap(), None, 404).unwrap_err();
    assert_eq!(error.kind, ReviewErrorKind::NotFound);
    assert!(error.message.ends_with("TF401180: The requested pull request was not found."), "{}", error.message);
}
