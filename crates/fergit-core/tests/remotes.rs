//! Integration tests for what `fergit_core::repo` offers code that talks to a repository's host:
//! its remotes, whether commits are present, and credentials from git's helpers.

mod common;

use std::path::Path;

use common::{Fixture, T0, commit, git};
use fergit_core::Oid;
use fergit_core::repo::{CredentialError, Remote, Repo};

fn remote(name: &str, url: &str) -> Remote {
    Remote { name: name.to_owned(), url: url.to_owned() }
}

/// Makes `repo` use git's plain-text `store` helper with `file`, holding `lines`.
fn use_store(repo: &Path, file: &Path, lines: &str) {
    std::fs::write(file, lines).unwrap();
    let helper = format!("store --file={}", file.display().to_string().replace('\\', "/"));
    git(repo, &["config", "credential.helper", &helper]);
}

#[test]
fn lists_remotes_in_configuration_order_as_configured_now() {
    let fixture = Fixture::new();
    let path = fixture.init("repo");
    git(&path, &["remote", "add", "origin", "https://dev.azure.com/contoso/Web/_git/site"]);
    let repo = Repo::open(&path).unwrap();
    // Added after opening: read from disk, not from the configuration loaded at open.
    git(&path, &["remote", "add", "fork", "git@ssh.dev.azure.com:v3/contoso/Web/site"]);
    git(&path, &["config", "remote.nourl.fetch", "+refs/heads/*:refs/remotes/nourl/*"]);

    assert_eq!(
        repo.remotes().unwrap(),
        [
            remote("origin", "https://dev.azure.com/contoso/Web/_git/site"),
            remote("fork", "git@ssh.dev.azure.com:v3/contoso/Web/site"),
        ]
    );
}

#[test]
fn tells_whether_commits_are_present() {
    let fixture = Fixture::new();
    let path = fixture.init("repo");
    let first = commit(&path, "a.txt", "a", "First", T0);
    let repo = Repo::open(&path).unwrap();
    let second = commit(&path, "a.txt", "b", "Second", T0 + 60);
    let missing: Oid = "1234567890123456789012345678901234567890".parse().unwrap();

    assert!(repo.has_objects(&[first, second]), "a commit made after opening is found too");
    assert!(!repo.has_objects(&[first, missing]));
    assert!(repo.has_objects(&[]));
}

#[test]
fn credentials_come_from_the_helpers_and_a_refused_one_is_forgotten() {
    let fixture = Fixture::new();
    let path = fixture.init("repo");
    let store = fixture.path("credentials");
    use_store(&path, &store, "https://ada:t0ken@dev.azure.com\n");
    let repo = Repo::open(&path).unwrap();
    let url = "https://dev.azure.com/contoso/Web/_git/site";

    let credential = repo.credential(url, None).unwrap();
    assert_eq!((credential.username.as_str(), credential.password.as_str()), ("ada", "t0ken"));

    repo.credential_worked(&credential, true);
    assert!(std::fs::read_to_string(&store).unwrap().contains("t0ken"), "an approved credential stays stored");

    repo.credential_worked(&credential, false);
    assert!(!std::fs::read_to_string(&store).unwrap().contains("t0ken"), "a refused credential is erased");
    assert!(matches!(repo.credential(url, None), Err(CredentialError::Unavailable(_))));
}

#[test]
fn without_a_helper_or_a_prompt_there_is_no_credential() {
    let fixture = Fixture::new();
    let path = fixture.init("repo");
    let repo = Repo::open(&path).unwrap();

    let error = repo.credential("https://me@dev.azure.com/contoso/Web/_git/site", None).unwrap_err();
    assert_eq!(error, CredentialError::Unavailable("FerGit has no credentials for dev.azure.com.".into()));
}
