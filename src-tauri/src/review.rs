//! Pull request review for open repositories: thin commands over [`Reviews`].
//!
//! These reject with a [`ReviewError`] rather than an `AppError`: a host that can't be reached or
//! wants another sign-in is an expected state the review view shows in place, with a way to retry.
//! Credential prompts reach the user the way operations' do, through `credentialPrompt` events.

use std::sync::Arc;

use fergit_core::review::{
    PullRequest, PullRequestQuery, PullRequestSummary, ReviewAction, ReviewError, ReviewErrorKind, ReviewHost, Reviews,
};
use fergit_core::session::Session;
use fergit_core::{OpId, SessionId};
use tauri::State;

use crate::AppState;
use crate::operations::Operations;

fn session(state: &AppState, id: SessionId) -> Result<Arc<Session>, ReviewError> {
    state.sessions.get(id).ok_or_else(|| ReviewError {
        kind: ReviewErrorKind::NotFound,
        message: "That repository is no longer open.".to_owned(),
    })
}

async fn blocking<T: Send + 'static>(work: impl FnOnce() -> Result<T, ReviewError> + Send + 'static) -> Result<T, ReviewError> {
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| ReviewError {
        kind: ReviewErrorKind::Server,
        message: format!("background task failed: {e}"),
    })?
}

/// Where the pull requests of `session`'s repository are, if a remote is on Azure DevOps. Reads the
/// repository's configuration only: nothing is contacted and no credential helper runs.
#[tauri::command]
#[specta::specta]
pub async fn review_host(state: State<'_, AppState>, session: SessionId) -> Result<Option<ReviewHost>, ReviewError> {
    let session = self::session(&state, session)?;
    blocking(move || Ok(Reviews::host(session.repo()))).await
}

/// The pull requests `query` selects. Signs in first if needed, which may prompt.
#[tauri::command]
#[specta::specta]
pub async fn pull_requests(
    state: State<'_, AppState>,
    operations: State<'_, Operations>,
    session: SessionId,
    query: PullRequestQuery,
) -> Result<Vec<PullRequestSummary>, ReviewError> {
    let (session, reviews, askpass) = (self::session(&state, session)?, Arc::clone(&state.reviews), operations.askpass());
    blocking(move || reviews.pull_requests(session.repo(), askpass.as_ref(), query)).await
}

/// Pull request `number`, with its reviewers and discussion.
#[tauri::command]
#[specta::specta]
pub async fn pull_request(
    state: State<'_, AppState>,
    operations: State<'_, Operations>,
    session: SessionId,
    number: u32,
) -> Result<PullRequest, ReviewError> {
    let (session, reviews, askpass) = (self::session(&state, session)?, Arc::clone(&state.reviews), operations.askpass());
    blocking(move || reviews.pull_request(session.repo(), askpass.as_ref(), number)).await
}

/// Does `action` to pull request `number`. `id` identifies the request: an id already acted on is
/// answered with that outcome instead of acting again. Read the pull request again afterwards.
#[tauri::command]
#[specta::specta]
pub async fn review(
    state: State<'_, AppState>,
    operations: State<'_, Operations>,
    session: SessionId,
    id: OpId,
    number: u32,
    action: ReviewAction,
) -> Result<(), ReviewError> {
    let (session, reviews, askpass) = (self::session(&state, session)?, Arc::clone(&state.reviews), operations.askpass());
    blocking(move || reviews.act(session.repo(), askpass.as_ref(), id, number, &action)).await
}
