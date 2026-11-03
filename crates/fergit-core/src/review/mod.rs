//! Reviewing pull requests on Azure DevOps: listing them, reading one with its reviewers and
//! discussion, voting, and commenting.
//!
//! Like everything else in FerGit, what this shows is derived data: every read asks the host, and
//! after every action the UI reads the pull request again rather than patching what it showed.
//! The changes of a pull request are not downloaded through the API: they are read from the local
//! repository, between the commit its branches last had in common and its latest commit, with the
//! same diff machinery as everywhere else. Fetch them with [`crate::Operation::FetchPullRequest`].
//!
//! # Signing in
//!
//! FerGit stores no credentials here either. The first call to an organization asks git's
//! credential helpers for the credential git would use to fetch from the remote (Git Credential
//! Manager's Microsoft sign-in, or a personal access token it keeps), and FerGit's own prompt when
//! none has one. The credential is then kept in memory, for that organization, until the host
//! refuses it. It is sent only to the organization's own `https` URL, never across a redirect.
//!
//! Every call that can prompt runs because the user opened the pull requests or acted on one, never
//! in the background: asking the helpers runs whatever the repository's configuration names.

mod model;
mod remote;
mod wire;

pub use model::*;

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use reqwest::Method;
use reqwest::blocking::Client;
use reqwest::header::ACCEPT;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use self::remote::AzureRepo;
use self::wire::API_VERSION;
use crate::askpass::Askpass;
use crate::repo::{Credential, CredentialError, Repo};
use crate::types::OpId;

/// How many finished actions are remembered to recognize a repeated id, as for operations.
const REMEMBERED: usize = 64;
/// The most pull requests one list shows.
const LIST_LIMIT: u32 = 100;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Pull request review for every open repository, sharing sign-ins by organization. All methods
/// block on the network; call them off the UI thread.
#[derive(Default)]
pub struct Reviews {
    /// Made on first use, on a thread that may block.
    http: OnceLock<Result<Client, String>>,
    /// By organization (collection) URL.
    signins: Mutex<HashMap<String, Signin>>,
    /// Held while asking the helpers, so concurrent calls prompt the user once, not once each.
    signing_in: Mutex<()>,
    /// Outcomes of the latest actions, by id; held while one runs, so actions run one at a time.
    acted: Mutex<VecDeque<(OpId, Result<(), ReviewError>)>>,
}

#[derive(Clone)]
struct Signin {
    credential: Credential,
    /// The host has accepted it (and the helpers were told so).
    accepted: bool,
    /// Who it signs in as, once asked.
    me: Option<Person>,
}

impl Reviews {
    pub fn new() -> Reviews {
        Reviews::default()
    }

    /// Where `repo`'s pull requests are: the Azure DevOps repository `origin` names, or else the
    /// first remote naming one. Reads the configuration only.
    pub fn host(repo: &Repo) -> Option<ReviewHost> {
        let (remote, azure) = find_host(repo)?;
        Some(ReviewHost {
            remote,
            organization: azure.organization().to_owned(),
            project: azure.project.clone(),
            repository: azure.repository.clone(),
        })
    }

    /// The pull requests `query` selects, newest first (at most 100).
    pub fn pull_requests(
        &self,
        repo: &Repo,
        askpass: Option<&Askpass>,
        query: PullRequestQuery,
    ) -> Result<Vec<PullRequestSummary>, ReviewError> {
        let host = self.connect(repo, askpass)?;
        let status = match query {
            PullRequestQuery::Completed => "completed",
            PullRequestQuery::Abandoned => "abandoned",
            _ => "active",
        };
        let mut url = format!(
            "{}/pullrequests?searchCriteria.status={status}&$top={LIST_LIMIT}&api-version={API_VERSION}",
            host.azure.api()
        );
        match query {
            PullRequestQuery::Mine => url.push_str(&format!("&searchCriteria.creatorId={}", host.me()?.id)),
            PullRequestQuery::Reviewing => url.push_str(&format!("&searchCriteria.reviewerId={}", host.me()?.id)),
            _ => {}
        }
        let list: wire::List<wire::PullRequest> = host.get(&url)?;
        Ok(list.value.iter().map(wire::summary).collect())
    }

    /// Pull request `number` with its reviewers and discussion, and whether its commits are here.
    pub fn pull_request(&self, repo: &Repo, askpass: Option<&Askpass>, number: u32) -> Result<PullRequest, ReviewError> {
        let host = self.connect(repo, askpass)?;
        let api = format!("{}/pullRequests/{number}", host.azure.api());
        let pr: wire::PullRequest = host.get(&format!("{api}?api-version={API_VERSION}"))?;
        let iterations: wire::List<wire::Iteration> = host.get(&format!("{api}/iterations?api-version={API_VERSION}"))?;
        let latest = iterations.value.iter().max_by_key(|iteration| iteration.id);
        let iteration = latest.map_or(0, |latest| latest.id);
        // Threads' positions, tracked to the latest iteration: the lines of the changes shown.
        let mut threads_url = format!("{api}/threads?api-version={API_VERSION}");
        if iteration > 0 {
            threads_url.push_str(&format!("&$iteration={iteration}"));
        }
        let threads: wire::List<wire::CommentThread> = host.get(&threads_url)?;
        let base = latest.and_then(|latest| wire::commit_id(latest.common_ref_commit.as_ref()));
        let head = latest
            .and_then(|latest| wire::commit_id(latest.source_ref_commit.as_ref()))
            .or_else(|| wire::commit_id(pr.last_merge_source_commit.as_ref()));
        let have_commits = match (base, head) {
            (Some(base), Some(head)) => repo.has_objects(&[base, head]),
            _ => false,
        };
        Ok(PullRequest {
            summary: wire::summary(&pr),
            description: pr.description.clone(),
            web_url: host.azure.web_url(number),
            me: host.me()?,
            merge_status: wire::merge_status(&pr),
            iteration,
            base,
            head,
            have_commits,
            threads: wire::threads(&threads.value),
        })
    }

    /// Does `action` to pull request `number` as the signed-in user. `id` identifies the request:
    /// an id already acted on returns that outcome without acting again, so a double click posts
    /// one comment. Read the pull request again afterwards to see the result.
    pub fn act(
        &self,
        repo: &Repo,
        askpass: Option<&Askpass>,
        id: OpId,
        number: u32,
        action: &ReviewAction,
    ) -> Result<(), ReviewError> {
        let mut acted = lock(&self.acted);
        if let Some((_, outcome)) = acted.iter().find(|(done, _)| *done == id) {
            return outcome.clone();
        }
        let outcome = self.perform(repo, askpass, number, action);
        if acted.len() == REMEMBERED {
            acted.pop_front();
        }
        acted.push_back((id, outcome.clone()));
        outcome
    }

    fn perform(&self, repo: &Repo, askpass: Option<&Askpass>, number: u32, action: &ReviewAction) -> Result<(), ReviewError> {
        let host = self.connect(repo, askpass)?;
        let api = format!("{}/pullRequests/{number}", host.azure.api());
        let written = |text: &str| match text.trim() {
            "" => Err(ReviewError::new(ReviewErrorKind::Refused, "Write a comment first.")),
            _ => Ok(()),
        };
        match action {
            ReviewAction::Vote { vote } => {
                let me = host.me()?;
                let url = format!("{api}/reviewers/{}?api-version={API_VERSION}", remote::encode(&me.id));
                host.send(Method::PUT, &url, &json!({ "vote": wire::vote_value(*vote), "id": me.id }))
            }
            ReviewAction::Comment { text, anchor, iteration } => {
                written(text)?;
                // Lets the host follow the thread through later pushes. Without it the thread still
                // lands where it was put, so failing to look it up isn't worth failing the comment.
                let change = match anchor {
                    Some(anchor) if *iteration > 0 => host.change_tracking_id(&api, *iteration, &anchor.path),
                    _ => None,
                };
                let body = wire::new_thread(text, anchor.as_ref(), *iteration, change);
                host.send(Method::POST, &format!("{api}/threads?api-version={API_VERSION}"), &body)
            }
            ReviewAction::Reply { thread, parent, text } => {
                written(text)?;
                let body = json!({ "content": text, "parentCommentId": parent, "commentType": 1 });
                host.send(Method::POST, &format!("{api}/threads/{thread}/comments?api-version={API_VERSION}"), &body)
            }
            ReviewAction::SetStatus { thread, status } => {
                let body = json!({ "status": wire::thread_status_value(*status) });
                host.send(Method::PATCH, &format!("{api}/threads/{thread}?api-version={API_VERSION}"), &body)
            }
        }
    }

    fn connect<'a>(&'a self, repo: &'a Repo, askpass: Option<&'a Askpass>) -> Result<Host<'a>, ReviewError> {
        let (_, azure) = find_host(repo)
            .ok_or_else(|| ReviewError::new(ReviewErrorKind::NoHost, "None of this repository's remotes is on Azure DevOps."))?;
        let http = self
            .http
            .get_or_init(|| {
                Client::builder()
                    .user_agent(concat!("FerGit/", env!("CARGO_PKG_VERSION")))
                    // Credentials go to the organization's URL only, so redirects aren't followed.
                    .redirect(reqwest::redirect::Policy::none())
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT)
                    .build()
                    .map_err(|err| err.to_string())
            })
            .as_ref()
            .map_err(|err| ReviewError::new(ReviewErrorKind::Server, format!("Can't set up HTTPS: {err}")))?;
        Ok(Host { reviews: self, http, repo, askpass, azure })
    }
}

/// The remote whose Azure DevOps repository holds the pull requests, and that repository.
fn find_host(repo: &Repo) -> Option<(String, AzureRepo)> {
    let remotes = repo.remotes().ok()?;
    let azure = |remote: &crate::repo::Remote| remote::parse(&remote.url).map(|azure| (remote.name.clone(), azure));
    remotes.iter().filter(|remote| remote.name == "origin").find_map(azure).or_else(|| remotes.iter().find_map(azure))
}

/// One call's way to the host.
struct Host<'a> {
    reviews: &'a Reviews,
    http: &'a Client,
    repo: &'a Repo,
    askpass: Option<&'a Askpass>,
    azure: AzureRepo,
}

enum Answer {
    Body(String),
    /// The host wants other credentials.
    SignIn,
}

impl Host<'_> {
    fn get<T: DeserializeOwned>(&self, url: &str) -> Result<T, ReviewError> {
        let body = self.request(Method::GET, url, None)?;
        serde_json::from_str(&body).map_err(|err| {
            ReviewError::new(ReviewErrorKind::Server, format!("{} answered something FerGit can't read: {err}", self.name()))
        })
    }

    fn send(&self, method: Method, url: &str, body: &Value) -> Result<(), ReviewError> {
        self.request(method, url, Some(body)).map(drop)
    }

    /// Sends a request signed in, signing in first if needed. A refused credential that came from
    /// the cache may just have expired, so it is retried once with a fresh one from the helpers; a
    /// fresh one that is refused is reported to them as refused.
    fn request(&self, method: Method, url: &str, body: Option<&Value>) -> Result<String, ReviewError> {
        loop {
            let signin = self.signin()?;
            match self.attempt(&signin.credential, method.clone(), url, body)? {
                Answer::Body(text) => {
                    if !signin.accepted {
                        self.accepted(&signin.credential);
                    }
                    return Ok(text);
                }
                Answer::SignIn => {
                    self.forget(&signin.credential);
                    if !signin.accepted {
                        self.repo.credential_worked(&signin.credential, false);
                        return Err(ReviewError::new(
                            ReviewErrorKind::SignIn,
                            format!("{} refused the credentials FerGit was given. Try again to enter others.", self.name()),
                        ));
                    }
                }
            }
        }
    }

    fn attempt(&self, credential: &Credential, method: Method, url: &str, body: Option<&Value>) -> Result<Answer, ReviewError> {
        let mut request = self
            .http
            .request(method, url)
            .header(ACCEPT, "application/json")
            // Answer a missing or refused sign-in with 401, not a redirect to a sign-in page.
            .header("X-TFS-FedAuthRedirect", "Suppress");
        // Microsoft sign-in (Git Credential Manager's default) gives a JWT, which is a bearer token;
        // anything else is a personal access token or password, sent like git sends it.
        request = if is_jwt(&credential.password) {
            request.bearer_auth(&credential.password)
        } else {
            request.basic_auth(&credential.username, Some(&credential.password))
        };
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().map_err(|err| {
            ReviewError::new(ReviewErrorKind::Unreachable, format!("Can't reach {}: {}", self.name(), cause(&err)))
        })?;
        let status = response.status();
        // 203 is the sign-in page some endpoints serve instead of a 401.
        if status.as_u16() == 401 || status.as_u16() == 203 || status.is_redirection() {
            return Ok(Answer::SignIn);
        }
        let text = response.text().map_err(|err| {
            ReviewError::new(ReviewErrorKind::Unreachable, format!("Lost the connection to {}: {}", self.name(), cause(&err)))
        })?;
        if status.is_success() {
            return Ok(Answer::Body(text));
        }
        let (kind, fallback) = match status.as_u16() {
            403 => (ReviewErrorKind::Forbidden, "You aren't allowed to do that here."),
            404 => (ReviewErrorKind::NotFound, "That isn't there any more."),
            400..=499 => (ReviewErrorKind::Refused, "The request was refused."),
            _ => (ReviewErrorKind::Server, "The server failed."),
        };
        let message = wire::error_message(&text).unwrap_or_else(|| format!("{fallback} (HTTP {})", status.as_u16()));
        Err(ReviewError::new(kind, format!("{}: {message}", self.name())))
    }

    /// The organization's sign-in, from memory or else from git's credential helpers.
    fn signin(&self) -> Result<Signin, ReviewError> {
        let key = &self.azure.collection;
        if let Some(signin) = lock(&self.reviews.signins).get(key) {
            return Ok(signin.clone());
        }
        let _one_at_a_time = lock(&self.reviews.signing_in);
        if let Some(signin) = lock(&self.reviews.signins).get(key) {
            return Ok(signin.clone());
        }
        let credential = self.repo.credential(&self.azure.credential_url, self.askpass).map_err(|err| match err {
            CredentialError::Unavailable(message) => ReviewError::new(
                ReviewErrorKind::SignIn,
                format!(
                    "{message} Sign in to Azure DevOps with git once (a fetch from a terminal), or enter a personal \
                     access token with Code (Read & write) access when FerGit asks for a password."
                ),
            ),
            CredentialError::GitNotFound(_) => ReviewError::new(ReviewErrorKind::Server, err.to_string()),
        })?;
        let signin = Signin { credential, accepted: false, me: None };
        lock(&self.reviews.signins).insert(key.clone(), signin.clone());
        Ok(signin)
    }

    fn accepted(&self, credential: &Credential) {
        let mut signins = lock(&self.reviews.signins);
        if let Some(signin) = signins.get_mut(&self.azure.collection).filter(|signin| signin.credential == *credential) {
            signin.accepted = true;
        }
        drop(signins);
        self.repo.credential_worked(credential, true);
    }

    fn forget(&self, credential: &Credential) {
        let mut signins = lock(&self.reviews.signins);
        if signins.get(&self.azure.collection).is_some_and(|signin| signin.credential == *credential) {
            signins.remove(&self.azure.collection);
        }
    }

    /// Who the credential signs in as.
    fn me(&self) -> Result<Person, ReviewError> {
        if let Some(me) = lock(&self.reviews.signins).get(&self.azure.collection).and_then(|signin| signin.me.clone()) {
            return Ok(me);
        }
        let data: wire::ConnectionData = self.get(&format!("{}/_apis/connectionData", self.azure.collection))?;
        let me = data.authenticated_user.person();
        if me.id.is_empty() {
            return Err(ReviewError::new(ReviewErrorKind::SignIn, format!("{} didn't say who you are signed in as.", self.name())));
        }
        if let Some(signin) = lock(&self.reviews.signins).get_mut(&self.azure.collection) {
            signin.me = Some(me.clone());
        }
        Ok(me)
    }

    /// The id the host follows `path` by through iteration `iteration`'s changes, if it lists it.
    fn change_tracking_id(&self, api: &str, iteration: u32, path: &str) -> Option<u32> {
        let url = format!("{api}/iterations/{iteration}/changes?$top=2000&api-version={API_VERSION}");
        let changes: wire::IterationChanges = self.get(&url).ok()?;
        let path = wire::host_path(path);
        changes.change_entries.iter().find(|entry| entry.item.path == path).map(|entry| entry.change_tracking_id)
    }

    /// The organization, for messages.
    fn name(&self) -> &str {
        self.azure.organization()
    }
}

/// Whether `token` looks like a JSON Web Token: three base64url parts, the first a JSON object.
fn is_jwt(token: &str) -> bool {
    token.starts_with("eyJ") && token.split('.').count() == 3
}

/// The innermost cause of a request error, which says what actually went wrong (a name that
/// doesn't resolve, a certificate that isn't trusted, a timeout).
fn cause(err: &reqwest::Error) -> String {
    let mut source: &dyn std::error::Error = err;
    while let Some(next) = source.source() {
        source = next;
    }
    if err.is_timeout() { "it took too long to answer".to_owned() } else { source.to_string() }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::is_jwt;

    #[test]
    fn tells_microsoft_sign_in_tokens_from_personal_access_tokens() {
        assert!(is_jwt("eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.eyJhdWQiOiI0OTkifQ.c2lnbmF0dXJl"));
        assert!(!is_jwt("4fmn2pvz7xg6q3qs5bhyjxbs2gktwaqyrdhkrg4l2fz2xvq7wkwa"));
        assert!(!is_jwt("eyJ-not-a-jwt"));
    }
}
