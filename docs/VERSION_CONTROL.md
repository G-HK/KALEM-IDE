# Version control and hosting platforms

Two separate layers, because "Git" and "GitHub" are different things.

## 1. Local version control (`VcsBackend`)

The tool that tracks files. **Git first**, behind a trait so others can follow.

| System | Plan |
|---|---|
| Git | v1. Through the real `git` executable, so the user's config, credential helpers, hooks, LFS, signing and submodules behave exactly as in a terminal |
| Jujutsu (jj), Sapling, Mercurial, SVN, Perforce, Fossil, Unity Version Control (Plastic) | Later, one backend each, behind the same trait. Capabilities differ, so the UI greys out what a backend cannot do |

Read paths (status, log, diff, blame) may later use a Rust library such as `gix` or `git2` for speed. Write paths
(commit, push, pull, rebase, merge, stash) stay on the `git` CLI. Verify library coverage before switching anything.

Status today: `GitCli::status`, `remote_url`. Everything else below is planned.

### Planned Git features
- Changes panel: stage/unstage files, hunks and single lines; commit, amend, sign-off, commit templates.
- Diff and 3-way merge editor; conflict resolution; inline blame; file history.
- Branches, tags, remotes; fetch, pull (merge or rebase), push, force-with-lease; stash; cherry-pick; revert.
- Interactive rebase; bisect; worktrees; submodules; LFS awareness; commit graph view.

## 2. Hosting platforms / forges (`forge`)

The service behind a remote. Detection from the remote URL works today (`vcs.forge`).

| Platform | Planned features |
|---|---|
| GitHub | Sign-in, clone, pull requests (list, create, review, merge), issues, Actions runs and manual `workflow_dispatch`, releases, checks |
| GitLab (incl. self-hosted) | Merge requests, pipelines, issues, releases |
| Bitbucket | Pull requests, pipelines, issues |
| Azure DevOps | Pull requests, pipelines, work items |
| Gitea / Forgejo / Codeberg | Pull requests, issues, actions |

Each platform is a provider plugin with a capability list, like language packs.

### Sign-in and credentials
- OAuth device flow where the platform supports it; personal access token or SSH key as fallback.
- Tokens are stored only in the OS keychain (Windows Credential Manager, macOS Keychain, Linux Secret Service).
  Never in settings files, never in logs, never in URLs.
- Reuse Git Credential Manager and ssh-agent when present. Support SSH and GPG commit signing.

## 3. Safety

- **Secret guard:** before every commit, scan the staged diff for secrets (keys, tokens, `.env` files) and block or warn.
  Pairs with the repository `.gitignore`.
- **Destructive commands** (force push, `reset --hard`, `clean -fd`, branch delete, rewriting pushed history) are
  flagged on the command and always need confirmation.
- **Agents:** commands invoked through MCP default to read-only VCS operations. Commit is allowed, push and
  anything destructive needs explicit human approval each time.

## 4. Commands (all dispatchable by UI, CLI and MCP)

Implemented: `vcs.status`, `vcs.forge`.
Planned: `vcs.stage`, `vcs.unstage`, `vcs.commit`, `vcs.diff`, `vcs.log`, `vcs.branch.*`, `vcs.fetch`, `vcs.pull`,
`vcs.push`, `vcs.stash.*`, `vcs.merge`, `vcs.rebase`, `forge.login`, `forge.pr.list`, `forge.pr.create`,
`forge.checks`, `forge.workflow.run`.

## 5. Phases

1. Git read and basic write through the CLI: status, stage, commit, diff, log, branches.
2. Sync and history: fetch, pull, push, stash, merge, conflict editor.
3. Forge sign-in, pull request list and create, CI status (GitHub first).
4. Other forges (GitLab, Bitbucket, Azure DevOps, Gitea/Forgejo).
5. Other VCS backends (Jujutsu, Mercurial, SVN, ...).
6. Advanced: interactive rebase, blame, worktrees, submodules, LFS, bisect.
