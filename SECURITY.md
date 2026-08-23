# Security policy

## Supported versions

Heretic is pre-1.0 and experimental. Only the latest release receives fixes.

| Version | Supported |
| ------- | --------- |
| latest  | ✅        |
| older   | ❌        |

## Reporting a vulnerability

Please **do not** open a public issue for a security problem.

Report it privately through GitHub's
[security advisory form](https://github.com/sirsjg/heretic/security/advisories/new).
You should get an acknowledgement within a few days, and an assessment of
whether it is a genuine issue shortly after that. Fixes ship in the next
release; you will be credited in the advisory unless you would rather not be.

## What Heretic touches on your machine

Worth knowing when judging whether something is a vulnerability:

- **It runs agent CLIs as your user.** `claude`, `codex` or whatever custom
  command you configure is executed as a subprocess with your permissions.
  Heretic does not sandbox them beyond whatever sandboxing that CLI provides
  itself. A prompt-injection-style attack that reaches an agent through Flux
  task text is a real concern — treat your board as trusted input.
- **It writes to git worktrees.** Agents work in `git worktree` checkouts under
  your repository, on their own branches. A merge into your base branch is
  refused while the main checkout has uncommitted changes.
- **It stores credentials locally.** Flux API keys, proxy service tokens and
  captured session cookies live in Heretic's config directory on disk, readable
  by your user. They are not encrypted at rest.
- **It talks to hosts you configure.** Flux servers and model hosts are
  contacted at the addresses you enter. Heretic warns before sending proxy
  credentials over plain `http://` to anything other than localhost.

## Release integrity

Every release publishes a `checksums.txt` alongside its artifacts. The install
script verifies the download against it before installing. Builds are produced
by the GitHub Actions workflow in this repository and are reproducible from the
tagged source.

macOS builds are **ad-hoc signed, not notarised.** See the README for what that
means in practice.
