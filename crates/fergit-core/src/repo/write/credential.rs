//! Credentials for talking to a git host directly, through git's own credential helpers.
//!
//! FerGit never stores credentials. To call a host's API (Azure DevOps, for reviewing pull
//! requests) it asks `git credential fill` for the user name and password git would send to the
//! same URL, which brings in Git Credential Manager, a keychain or whatever helper the user set up,
//! and FerGit's askpass prompt when none has one. Afterwards `git credential approve` or `reject`
//! tells the helpers whether the host took it, exactly as git does after a fetch, so a token typed
//! into the prompt is stored by the user's helper, and a refused one is forgotten.
//!
//! Git Credential Manager runs non-interactively here like everywhere in FerGit: it hands out what
//! it has (and refreshes OAuth tokens silently), but can't open a sign-in window.

use std::fmt;
use std::path::Path;

use super::git::Git;
use crate::askpass::Askpass;

/// A user name and password (or token) for one URL, as git's credential helpers supplied them.
/// Lives in memory only; `Debug` never shows the secret.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential {
    pub username: String,
    pub password: String,
    /// Everything `git credential fill` answered (`key=value` lines), handed back as-is on approval
    /// or rejection, as the credential protocol asks.
    description: String,
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credential").field("username", &self.username).field("password", &"***").finish()
    }
}

/// Why no credential could be had.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CredentialError {
    /// No helper had one and the prompt was cancelled, failed or isn't available.
    #[error("{0}")]
    Unavailable(String),
    /// Git couldn't be run at all.
    #[error("Can't run git: {0}. Install git and make sure it is on the PATH.")]
    GitNotFound(String),
}

/// Asks git's helpers, then the user through `askpass`, for the credential to use with `url`.
pub(in crate::repo) fn credential_fill(root: &Path, url: &str, askpass: Option<&Askpass>) -> Result<Credential, CredentialError> {
    let unavailable = || CredentialError::Unavailable(format!("FerGit has no credentials for {}.", host_of(url)));
    if url.contains(['\n', '\r', '\0']) {
        return Err(unavailable());
    }
    let git = Git { root, askpass };
    let output = git
        .run_unscrubbed(&["credential", "fill"], format!("url={url}\n\n").as_bytes())
        .map_err(|err| CredentialError::GitNotFound(err.to_string()))?;
    if !output.success() {
        return Err(unavailable());
    }
    let field = |key: &str| {
        output.stdout.lines().find_map(|line| line.strip_prefix(key).and_then(|rest| rest.strip_prefix('=')))
    };
    match (field("username"), field("password")) {
        (Some(username), Some(password)) if !password.is_empty() => Ok(Credential {
            username: username.to_owned(),
            password: password.to_owned(),
            description: output.stdout.clone(),
        }),
        _ => Err(unavailable()),
    }
}

/// Reports to git's helpers whether `credential` worked. Best effort: a helper that can't store
/// or forget it leaves things as they were, which only means the user may be asked again.
pub(in crate::repo) fn credential_verdict(root: &Path, credential: &Credential, worked: bool) {
    let action = if worked { "approve" } else { "reject" };
    let mut input = credential.description.trim_end().to_owned();
    input.push_str("\n\n");
    let _ = Git { root, askpass: None }.run_unscrubbed(&["credential", action], input.as_bytes());
}

/// The host part of `url`, for messages: never its user name or path.
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    authority.rsplit_once('@').map_or(authority, |(_, host)| host)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_password() {
        let credential = Credential { username: "me".into(), password: "s3cret".into(), description: "password=s3cret".into() };
        let shown = format!("{credential:?}");
        assert!(shown.contains("me") && !shown.contains("s3cret"), "{shown}");
    }

    #[test]
    fn messages_name_only_the_host() {
        assert_eq!(host_of("https://me@dev.azure.com/org/p/_git/r"), "dev.azure.com");
        assert_eq!(host_of("https://contoso.visualstudio.com/p/_git/r"), "contoso.visualstudio.com");
    }
}
