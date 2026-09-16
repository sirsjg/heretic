# Contributing to Heretic

Thanks for looking. Heretic is experimental and moves quickly, so the most
useful contributions are small and self-contained.

## Before you start

For anything larger than a bug fix, open an issue first and say what you have in
mind. It saves you building something that does not fit, and it saves both of us
a long review.

## Getting set up

You need [Rust](https://rustup.rs) (stable), [Node](https://nodejs.org) 20+ and
[pnpm](https://pnpm.io).

```bash
pnpm install
pnpm app          # development, with hot reload
```

On Linux you also need the usual Tauri system packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev build-essential curl wget file libssl-dev libxdo-dev
```

The interface also runs in a plain browser against a mock engine, which is the
fastest way to work on the UI:

```bash
pnpm dev          # http://localhost:5183
```

## Checks

CI runs exactly these. Run them before you push:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck
pnpm build
```

The engine (`crates/heretic-core`) is deliberately free of Tauri so orchestration
logic can be tested without launching a desktop app. Keep it that way — anything
that needs a window belongs in `crates/heretic-app`.

Everything an interface can ask the engine to do lives in
`heretic_core::Service`. The desktop shell (`crates/heretic-app`) and the
remote server (`crates/heretic-server`) are both thin over it: a Tauri command
and an HTTP call for the same operation are one-liners that hand to the same
method. A new capability goes in the service first, then gets a command and a
line in the server's dispatch table — never logic in a shell.

Both shells embed the built interface, so `pnpm build` has to run before the
Rust crates will compile (`tauri-build` and `rust-embed` both want `ui/dist`
to exist). The remote server can be exercised without the desktop app:

```bash
pnpm build
HERETIC_CONFIG_DIR=/tmp/heretic cargo run -p heretic-server --bin heretic-serve -- --port 7499
```

The interface picks its transport from where it is running: Tauri's IPC inside
the desktop window, HTTP when served by `heretic-server` (any built bundle in a
browser), and the mock engine under `pnpm dev`. That switch is the whole of
`ui/src/lib/bridge.ts`; nothing above it knows which one it is on.

## Commit messages

Releases are cut automatically by
[semantic-release](https://semantic-release.gitbook.io), which reads
[Conventional Commits](https://www.conventionalcommits.org). Your commit message
decides the next version number, so it matters:

| Prefix                                   | Result          |
| ---------------------------------------- | --------------- |
| `fix:`                                   | patch — `0.1.1` |
| `feat:`                                  | minor — `0.2.0` |
| `feat!:` or a `BREAKING CHANGE:` footer  | major — `1.0.0` |
| `docs:` `chore:` `ci:` `test:` `style:`  | no release      |
| `perf:` `refactor:`                      | patch           |

```
feat(runner): pass the host's real context window to Codex

Codex otherwise guesses at fallback metadata, which truncates long
briefs on models with a large window.
```

Scope is optional. Keep the subject in the imperative mood and under about 72
characters.

## Pull requests

- One concern per PR.
- Say what changed and why in the description — the *why* is the part a reviewer
  cannot reconstruct.
- If it changes behaviour anyone would notice, update the README in the same PR.
- CI must be green.

## Licence

Contributions are accepted under the [MIT Licence](LICENSE), the same terms the
project is released under.
