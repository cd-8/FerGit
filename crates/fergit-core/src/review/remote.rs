//! Recognizing an Azure DevOps repository from a remote's URL. Pure parsing: nothing is contacted.
//!
//! The forms recognized:
//! - `https://[user@]dev.azure.com/{org}/{project}/_git/{repo}`, and `…/{org}/_git/{repo}` when the
//!   repository is named after its project;
//! - `https://[user@]{org}.visualstudio.com/[DefaultCollection/][{project}/]_git/{repo}`;
//! - `git@ssh.dev.azure.com:v3/{org}/{project}/{repo}`, also as an `ssh://` URL, and the older
//!   `{org}@vs-ssh.visualstudio.com:v3/…`;
//! - Azure DevOps Server: `https://{host}/{collection path}/{project}/_git/{repo}`, or
//!   `https://{host}/{collection}/_git/{repo}` for a repository named after its project.
//!
//! Only `https` is used to reach a host, so credentials never travel in the clear; `http` is
//! accepted for loopback addresses alone (a local server, the tests).

/// An Azure DevOps repository, with what is needed to reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AzureRepo {
    /// The organization's or collection's URL, no trailing slash: `https://dev.azure.com/contoso`.
    pub collection: String,
    /// Project and repository names, decoded (`My Project`).
    pub project: String,
    pub repository: String,
    /// The URL git's credential helpers are asked about: the remote's own `https` URL, so the
    /// credential is the one fetching from it uses, or the organization's for an `ssh` remote.
    pub credential_url: String,
}

impl AzureRepo {
    /// The organization (or the collection, for a server) as the user knows it, for display.
    pub fn organization(&self) -> &str {
        let path = self.collection.split_once("://").map_or(&*self.collection, |(_, rest)| rest);
        match path.strip_prefix("dev.azure.com/") {
            Some(org) => org,
            None => path.strip_suffix(".visualstudio.com").unwrap_or(path),
        }
    }

    /// `{collection}/{project}/_apis/git/repositories/{repository}`, for the REST API.
    pub fn api(&self) -> String {
        format!("{}/{}/_apis/git/repositories/{}", self.collection, encode(&self.project), encode(&self.repository))
    }

    /// The page of pull request `number` in the web interface.
    pub fn web_url(&self, number: u32) -> String {
        format!("{}/{}/_git/{}/pullrequest/{number}", self.collection, encode(&self.project), encode(&self.repository))
    }
}

/// The Azure DevOps repository `url` names, if it names one.
pub(super) fn parse(url: &str) -> Option<AzureRepo> {
    let url = url.trim();
    if let Some(rest) = url.strip_prefix("https://") {
        return parse_http("https", rest);
    }
    if let Some(rest) = url.strip_prefix("http://") {
        return parse_http("http", rest).filter(|repo| is_loopback(&repo.collection));
    }
    parse_ssh(url)
}

fn parse_http(scheme: &str, rest: &str) -> Option<AzureRepo> {
    let (authority, path) = rest.split_once('/')?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host).to_ascii_lowercase();
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').collect();
    let git = segments.iter().position(|segment| *segment == "_git")?;
    let repository = decode(segments.get(git + 1)?);
    if repository.is_empty() || segments.len() != git + 2 {
        return None;
    }
    let before: Vec<String> = segments[..git].iter().map(|segment| decode(segment)).collect();
    if before.iter().any(String::is_empty) {
        return None;
    }
    let (collection, project): (Vec<String>, Option<String>) = if host == "dev.azure.com" {
        match before.as_slice() {
            [org] => (vec![org.clone()], None),
            [org, project] => (vec![org.clone()], Some(project.clone())),
            _ => return None,
        }
    } else if host.ends_with(".visualstudio.com") {
        match before.as_slice() {
            [] => (vec![], None),
            [first] if first.eq_ignore_ascii_case("DefaultCollection") => (vec![first.clone()], None),
            [project] => (vec![], Some(project.clone())),
            [first, project] if first.eq_ignore_ascii_case("DefaultCollection") => (vec![first.clone()], Some(project.clone())),
            _ => return None,
        }
    } else {
        // A server: the collection path always has at least one segment.
        match before.as_slice() {
            [] => return None,
            [collection] => (vec![collection.clone()], None),
            [collection @ .., project] => (collection.to_vec(), Some(project.clone())),
        }
    };
    let mut collection_url = format!("{scheme}://{host}");
    for segment in &collection {
        collection_url.push('/');
        collection_url.push_str(&encode(segment));
    }
    Some(AzureRepo {
        collection: collection_url,
        project: project.unwrap_or_else(|| repository.clone()),
        credential_url: format!("{scheme}://{rest}"),
        repository,
    })
}

fn parse_ssh(url: &str) -> Option<AzureRepo> {
    // `user@host:v3/…`, or `ssh://user@host[:port]/v3/…`.
    let (host, path) = match url.strip_prefix("ssh://") {
        Some(rest) => {
            let (authority, path) = rest.split_once('/')?;
            let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
            (host.split(':').next()?, path)
        }
        None => {
            let (authority, path) = url.split_once(':')?;
            (authority.rsplit_once('@')?.1, path)
        }
    };
    let host = host.to_ascii_lowercase();
    let [org, project, repository] = path.trim_end_matches('/').strip_prefix("v3/")?.split('/').collect::<Vec<_>>()[..] else {
        return None;
    };
    let (org, project, repository) = (decode(org), decode(project), decode(repository));
    if org.is_empty() || project.is_empty() || repository.is_empty() {
        return None;
    }
    let collection = match host.as_str() {
        "ssh.dev.azure.com" => format!("https://dev.azure.com/{}", encode(&org)),
        "vs-ssh.visualstudio.com" => format!("https://{}.visualstudio.com", encode(&org)),
        _ => return None,
    };
    Some(AzureRepo {
        credential_url: format!("{collection}/{}/_git/{}", encode(&project), encode(&repository)),
        collection,
        project,
        repository,
    })
}

fn is_loopback(url: &str) -> bool {
    let host = url.split_once("://").map_or(url, |(_, rest)| rest).split('/').next().unwrap_or("");
    let host = host.rsplit_once(':').map_or(host, |(host, port)| if port.chars().all(|c| c.is_ascii_digit()) { host } else { "" });
    matches!(host, "127.0.0.1" | "localhost" | "[::1]")
}

/// Undoes percent-encoding; malformed escapes are kept as they are.
fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = |b: u8| (b as char).to_digit(16);
        match (bytes[i], bytes.get(i + 1).copied().and_then(hex), bytes.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(high), Some(low)) => {
                out.push((high * 16 + low) as u8);
                i += 3;
            }
            (byte, ..) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encodes everything but unreserved characters, for one path segment.
pub(super) fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(collection: &str, project: &str, repository: &str, credential_url: &str) -> Option<AzureRepo> {
        Some(AzureRepo {
            collection: collection.into(),
            project: project.into(),
            repository: repository.into(),
            credential_url: credential_url.into(),
        })
    }

    #[test]
    fn recognizes_dev_azure_com() {
        let url = "https://contoso@dev.azure.com/contoso/My%20Project/_git/web-site";
        assert_eq!(parse(url), repo("https://dev.azure.com/contoso", "My Project", "web-site", url));
        let short = "https://dev.azure.com/contoso/_git/Tools";
        assert_eq!(parse(short), repo("https://dev.azure.com/contoso", "Tools", "Tools", short));
        // The credential URL stays exactly as configured: it is what git asks its helpers about.
        let slash = "https://dev.azure.com/contoso/Web/_git/site/";
        assert_eq!(parse(slash), repo("https://dev.azure.com/contoso", "Web", "site", slash));
    }

    #[test]
    fn recognizes_visualstudio_com() {
        let url = "https://contoso.visualstudio.com/Web/_git/site";
        assert_eq!(parse(url), repo("https://contoso.visualstudio.com", "Web", "site", url));
        let url = "https://contoso.visualstudio.com/DefaultCollection/Web/_git/site";
        assert_eq!(parse(url), repo("https://contoso.visualstudio.com/DefaultCollection", "Web", "site", url));
        let url = "https://contoso.visualstudio.com/_git/site";
        assert_eq!(parse(url), repo("https://contoso.visualstudio.com", "site", "site", url));
    }

    #[test]
    fn recognizes_ssh_remotes_and_asks_for_https_credentials() {
        let expected = repo(
            "https://dev.azure.com/contoso",
            "My Project",
            "site",
            "https://dev.azure.com/contoso/My%20Project/_git/site",
        );
        assert_eq!(parse("git@ssh.dev.azure.com:v3/contoso/My%20Project/site"), expected);
        assert_eq!(parse("ssh://git@ssh.dev.azure.com:22/v3/contoso/My%20Project/site"), expected);
        assert_eq!(
            parse("contoso@vs-ssh.visualstudio.com:v3/contoso/Web/site"),
            repo("https://contoso.visualstudio.com", "Web", "site", "https://contoso.visualstudio.com/Web/_git/site")
        );
    }

    #[test]
    fn recognizes_servers_by_their_git_path() {
        let url = "https://tfs.contoso.local/tfs/DefaultCollection/Web/_git/site";
        assert_eq!(parse(url), repo("https://tfs.contoso.local/tfs/DefaultCollection", "Web", "site", url));
        let url = "https://tfs.contoso.local:8443/Main/_git/Tools";
        assert_eq!(parse(url), repo("https://tfs.contoso.local:8443/Main", "Tools", "Tools", url));
    }

    #[test]
    fn plain_http_only_on_loopback() {
        assert_eq!(parse("http://tfs.contoso.local/tfs/Main/Web/_git/site"), None);
        let url = "http://127.0.0.1:8080/Main/Web/_git/site";
        assert_eq!(parse(url), repo("http://127.0.0.1:8080/Main", "Web", "site", url));
    }

    #[test]
    fn other_remotes_are_not_azure_devops() {
        for url in [
            "https://github.com/o/r.git",
            "git@github.com:o/r.git",
            "/srv/git/r.git",
            "https://dev.azure.com/contoso/a/b/_git/site",
            "https://dev.azure.com/contoso/Web/_git/",
            "https://dev.azure.com/contoso/Web/_git/site/extra",
            "git@ssh.dev.azure.com:v3/contoso/site",
            "https://example.com/_git/site",
        ] {
            assert_eq!(parse(url), None, "{url}");
        }
    }

    #[test]
    fn builds_api_and_web_urls_with_encoded_names() {
        let azure = parse("https://dev.azure.com/contoso/My%20Project/_git/site").unwrap();
        assert_eq!(azure.api(), "https://dev.azure.com/contoso/My%20Project/_apis/git/repositories/site");
        assert_eq!(azure.web_url(12), "https://dev.azure.com/contoso/My%20Project/_git/site/pullrequest/12");
        assert_eq!(azure.organization(), "contoso");
        assert_eq!(parse("https://contoso.visualstudio.com/Web/_git/site").unwrap().organization(), "contoso");
        assert_eq!(
            parse("https://tfs.contoso.local/tfs/Main/Web/_git/site").unwrap().organization(),
            "tfs.contoso.local/tfs/Main"
        );
    }
}
