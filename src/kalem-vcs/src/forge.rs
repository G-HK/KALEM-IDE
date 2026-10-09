//! Hosting platforms ("forges") behind a remote URL.
//!
//! Detection only for now. Later each forge becomes a provider with the
//! platform features: sign-in (OAuth device flow, tokens in the OS keychain),
//! pull/merge requests, reviews, issues, CI status, releases and running
//! workflows.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForgeKind {
    GitHub,
    GitLab,
    Bitbucket,
    AzureDevOps,
    Codeberg,
    /// Self-hosted Gitea / Forgejo (or anything we cannot identify by host).
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteRef {
    pub kind: ForgeKind,
    pub host: String,
    /// User/org (for Azure DevOps: `org/project`).
    pub owner: String,
    pub repo: String,
}

fn kind_for_host(host: &str) -> ForgeKind {
    match host {
        "github.com" => ForgeKind::GitHub,
        "gitlab.com" => ForgeKind::GitLab,
        "bitbucket.org" => ForgeKind::Bitbucket,
        "dev.azure.com" | "ssh.dev.azure.com" => ForgeKind::AzureDevOps,
        "codeberg.org" => ForgeKind::Codeberg,
        h if h.starts_with("gitlab.") => ForgeKind::GitLab,
        _ => ForgeKind::Other,
    }
}

/// Parse an https, ssh:// or scp-style (`git@host:owner/repo.git`) remote URL.
pub fn parse_remote(url: &str) -> Option<RemoteRef> {
    let url = url.trim();
    let (host, path) = if let Some(rest) = url.split_once("://").map(|(_, r)| r) {
        let rest = rest.split_once('@').map(|(_, r)| r).unwrap_or(rest); // strip user[:token]@
        let (host, path) = rest.split_once('/')?;
        (host.split(':').next()?.to_string(), path.to_string())
    } else {
        // scp-like: user@host:path
        let rest = url.split_once('@').map(|(_, r)| r).unwrap_or(url);
        let (host, path) = rest.split_once(':')?;
        (host.to_string(), path.to_string())
    };
    let path = path.trim_matches('/').trim_end_matches(".git");
    let kind = kind_for_host(&host);
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let (owner, repo) = match kind {
        ForgeKind::AzureDevOps => {
            // https://dev.azure.com/org/project/_git/repo  |  git@ssh.dev.azure.com:v3/org/project/repo
            let parts: Vec<&str> = parts
                .iter()
                .copied()
                .filter(|p| *p != "_git" && *p != "v3")
                .collect();
            if parts.len() < 3 {
                return None;
            }
            (format!("{}/{}", parts[0], parts[1]), parts[2].to_string())
        }
        _ => {
            if parts.len() < 2 {
                return None;
            }
            // GitLab subgroups: everything before the last segment is the owner.
            (
                parts[..parts.len() - 1].join("/"),
                parts[parts.len() - 1].to_string(),
            )
        }
    };
    Some(RemoteRef {
        kind,
        host,
        owner,
        repo,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(url: &str) -> RemoteRef {
        parse_remote(url).unwrap_or_else(|| panic!("failed to parse {url}"))
    }

    #[test]
    fn github_variants() {
        for url in [
            "https://github.com/me/kalem-ide.git",
            "https://github.com/me/kalem-ide",
            "git@github.com:me/kalem-ide.git",
            "ssh://git@github.com/me/kalem-ide.git",
            "https://user:token@github.com/me/kalem-ide.git",
        ] {
            let x = r(url);
            assert_eq!(x.kind, ForgeKind::GitHub, "{url}");
            assert_eq!(
                (x.owner.as_str(), x.repo.as_str()),
                ("me", "kalem-ide"),
                "{url}"
            );
        }
    }

    #[test]
    fn other_forges() {
        assert_eq!(
            r("https://gitlab.com/group/sub/proj.git").owner,
            "group/sub"
        );
        assert_eq!(
            r("https://gitlab.com/group/sub/proj.git").kind,
            ForgeKind::GitLab
        );
        assert_eq!(
            r("git@bitbucket.org:team/app.git").kind,
            ForgeKind::Bitbucket
        );
        assert_eq!(r("https://codeberg.org/a/b").kind, ForgeKind::Codeberg);
        assert_eq!(r("https://git.example.com/a/b.git").kind, ForgeKind::Other);
    }

    #[test]
    fn azure_devops() {
        let a = r("https://dev.azure.com/org/proj/_git/repo");
        assert_eq!(
            (a.kind, a.owner.as_str(), a.repo.as_str()),
            (ForgeKind::AzureDevOps, "org/proj", "repo")
        );
        let b = r("git@ssh.dev.azure.com:v3/org/proj/repo");
        assert_eq!((b.owner.as_str(), b.repo.as_str()), ("org/proj", "repo"));
    }

    #[test]
    fn garbage_is_none() {
        assert!(parse_remote("not a url").is_none());
        assert!(parse_remote("https://github.com/only-owner").is_none());
    }
}
