# Heretic + Flux = 🔥

[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/sirsjg/heretic?display_name=tag&sort=semver)](https://github.com/sirsjg/heretic/releases)
[![CI](https://github.com/sirsjg/heretic/actions/workflows/ci.yml/badge.svg)](https://github.com/sirsjg/heretic/actions/workflows/ci.yml)
[![Downloads](https://img.shields.io/github/downloads/sirsjg/heretic/total)](https://github.com/sirsjg/heretic/releases)
[![Conventional Commits](https://img.shields.io/badge/commits-conventional-fe5196?logo=conventionalcommits&logoColor=white)](https://www.conventionalcommits.org)
![Rust](https://img.shields.io/badge/Rust-000000?style=flat&logo=rust&logoColor=white)
![Tauri](https://img.shields.io/badge/Tauri-24C8DB?style=flat&logo=tauri&logoColor=white)
![React](https://img.shields.io/badge/React-20232A?style=flat&logo=react&logoColor=61DAFB)
![macOS](https://img.shields.io/badge/macOS-000000?style=flat&logo=apple&logoColor=white)
![Linux](https://img.shields.io/badge/Linux-FCC624?style=flat&logo=linux&logoColor=black)

> [!WARNING]
> This tool is experimental and not ready for production use.

**The perfect companion to [Flux](https://github.com/sirsjg/flux).** Point it at
a project, switch **Auto** on for an epic, and it works the ready tasks with a
team of AI agents — one to plan, one to implement, one to review, one to
document.

Each role is bound to a model you choose, so the expensive judgement can sit with
a strong hosted model while the implementation grind runs on a local one.

<br>

## Heretic replaces Momentum

[Momentum](https://github.com/sirsjg/momentum) was the first companion to Flux:
a terminal UI that watched the board and threw a single Claude Code agent at
each ready task. It worked, and it is where these ideas were proved — but one
agent per task means one model, one pass, and nobody checking the work.

Heretic is the replacement. Same premise, different machine underneath:

| | Momentum | **Heretic** |
|---|---|---|
| Interface | Terminal UI | Desktop app (macOS, Linux) |
| Per task | One agent, one pass | A team — plan → implement → review → document |
| Models | Claude Code only | Any mix: Claude Code, Codex, OpenCode, local models, custom CLIs |
| Review | None | A reviewer that can send work back with notes |
| Isolation | Shared checkout | A `git worktree` per task, on its own branch |
| Landing the work | Committed in place | Merge or discard from the app, on your terms |
| Local models | — | Ollama, vLLM, LM Studio, llama.cpp — local or over the network |

Momentum still works and its releases stay up, but new work happens here. If you
are starting today, start with Heretic.

<br>

## What it does

- **Reads your real board.** Projects, epics and tasks come straight from Flux,
  with its acceptance criteria and guardrails carried into every prompt.
- **Respects the Auto switch.** A task only runs unattended when its epic has
  `auto` on, its epic dependencies are finished, and the task itself is
  unblocked. Everything else waits for you to press Run.
- **Runs a team, not a single agent.** A run goes
  plan → implement → review → document → integrate. A reviewer that requests
  changes sends the work back to the implementer with its notes attached.
- **Keeps agents out of each other's way.** Each task gets its own
  `git worktree` on its own branch, so several can run at once without
  colliding.
- **Shows you the work, not just the transcript.** Beside the run's feed sits
  the diff it produced — every file with its own patch, and the commits it put
  on its branch — read straight from git, so you can decide whether to merge
  without leaving for a terminal.
- **Writes back what happened.** Status transitions and a summary comment go to
  Flux under the agent's name, so the board stays honest whether you are
  watching or not.
- **Lets agents ask — if you allow it.** Yolo mode (the default) means agents
  never stop to ask you anything. Switch it off for a project and an agent that
  is genuinely blocked can pause its run with a question; the run waits, marked
  *Waiting for you*, until you answer, and the stage then reruns with your
  answer in hand.
- **Puts the thinking where you want it.** Each model profile can set a
  reasoning effort — a thinking budget for Claude Code, `model_reasoning_effort`
  for Codex, `reasoningEffort` for OpenCode hosts — so the reviewer can think
  hard while the implementation grind stays fast.

<br>

## How a run works

```
   Flux board                   Heretic                      Your repo
┌───────────────┐         ┌──────────────────┐          ┌──────────────────┐
│ epic: auto ✓  │────────▶│ 1. is it ready?  │          │                  │
│ task: todo    │         │ 2. worktree      │─────────▶│ heretic/<task>   │
│  · criteria   │         │ 3. plan          │          │   ┌────────────┐ │
│  · guardrails │         │ 4. implement     │          │   │ agent works│ │
└───────────────┘         │ 5. review ──┐    │          │   └────────────┘ │
        ▲                 │      ▲      │    │          │                  │
        │                 │      └──────┘    │          └──────────────────┘
        │                 │   changes        │
        │                 │   requested      │
        └─────────────────│ 6. commit + done │
          status + comment└──────────────────┘
```

### Seeing what one agent told the next

Each stage is a hand-off: the planner's brief goes to the implementer, the diff
and the reviewer's notes come back to it. Heretic writes those prompts, not the
agents, so the run feed records each one under a **Generated prompt** tag,
folded away until you open it. That is where you read what the implementer was
actually asked to do, or which of the reviewer's points made it into the
revision — the command line logged beside it deliberately shows `'<prompt>'`
instead, so the flags stay legible.

### What happens to the work

A finished run leaves its commits on its own branch, in its own worktree. What
happens next depends on the project's **When a run is approved** setting:

- **Leave the branch for me to review** (the default) — the run finishes with
  the work committed and nothing touched on your base branch. The run shows
  *Not merged*, with **Merge** and **Discard** beside it. Merging brings the
  branch in and removes the worktree; discarding deletes both.
- **Merge it into the base branch** — the same merge happens automatically as
  the last stage of the run.

Either way the merge is refused while your main checkout has uncommitted
changes, so a run can never overwrite work in progress.

Finished tasks are clickable on the board and open their run, so reviewing what
an agent did does not mean hunting through the Runs list. A task whose work is
still sitting on a branch is marked *Not merged* there too.

Nothing is marked done until the reviewer approves it. If a run fails, is
stopped, or never satisfies review, the task goes back to **Todo** with a
blocker recorded on the Flux board — which explains what happened and stops the
auto loop from retrying the same task forever.

An unreadable review verdict is never treated as approval.

<br>

## Requirements

- **[Flux](https://github.com/sirsjg/flux)** running and reachable
  (default `http://localhost:3000`). Flux servers are locked by default, so you
  will normally need an API key.
- **At least one agent CLI** on your `PATH`:
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code) — `claude`
  - [Codex](https://github.com/openai/codex) — `codex`, including `--oss` mode
    for local models through [Ollama](https://ollama.com)
  - [OpenCode](https://opencode.ai) — `opencode`, against its own providers or
    a model server of your own
  - or any other agent CLI, described as a custom command
- **git**, and a repository for each project you want worked.

<br>

## Install

### Installer (macOS & Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/sirsjg/heretic/main/install.sh | sh
```

It detects your operating system and architecture, verifies the download against
the release checksum, and then installs it — `Heretic.app` into `/Applications`
on macOS, or the AppImage into `~/.local/bin` with a desktop entry on Linux.

Set `HERETIC_VERSION` to pin a release, `HERETIC_APP_DIR` (macOS) or
`HERETIC_INSTALL_DIR` (Linux) to install somewhere else:

```bash
curl -fsSL https://raw.githubusercontent.com/sirsjg/heretic/main/install.sh | \
  HERETIC_VERSION=0.1.0 HERETIC_APP_DIR="$HOME/Applications" sh
```

### Homebrew (macOS)

```bash
brew install --cask --no-quarantine sirsjg/heretic/heretic
```

`--no-quarantine` is needed because Heretic is not notarised — see below.

### Manually

Grab a bundle from the [releases page](https://github.com/sirsjg/heretic/releases):

| Platform | File |
|---|---|
| macOS, Apple Silicon | `heretic_<version>_darwin_arm64.dmg` |
| macOS, Intel | `heretic_<version>_darwin_amd64.dmg` |
| Linux, x86_64 | `heretic_<version>_linux_amd64.AppImage` |
| Debian / Ubuntu, x86_64 | `heretic_<version>_linux_amd64.deb` |

```bash
# Debian and derivatives
sudo apt install ./heretic_0.1.0_linux_amd64.deb

# AppImage anywhere else
chmod +x heretic_0.1.0_linux_amd64.AppImage && ./heretic_0.1.0_linux_amd64.AppImage
```

Every release ships a `checksums.txt`. Verify before you run anything:

```bash
sha256sum -c checksums.txt --ignore-missing
```

Linux on ARM has no prebuilt bundle yet — build from source below.

### A note on macOS Gatekeeper

Heretic is **ad-hoc signed but not notarised**, because notarisation needs a
paid Apple Developer ID. That has no effect on how the app behaves, but it does
change how you are allowed to open it:

- **The installer script is fine.** `curl` does not set the quarantine flag, and
  the script clears it anyway.
- **A browser download is quarantined.** macOS will refuse to open it. Clear the
  flag once:

  ```bash
  xattr -dr com.apple.quarantine /Applications/Heretic.app
  ```

- **Homebrew** applies quarantine unless you pass `--no-quarantine`.

If that trade is not one you want to make, build from source — the result is
identical.

<br>

## Build from source

Needs [Rust](https://rustup.rs) (stable), [Node](https://nodejs.org) 20+ and
[pnpm](https://pnpm.io).

```bash
git clone https://github.com/sirsjg/heretic
cd heretic
pnpm install
pnpm app          # development, with hot reload
pnpm app:build    # a bundled .app / .dmg / .deb / .AppImage
```

On Linux you also need the usual Tauri system packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev libssl-dev libxdo-dev patchelf build-essential curl wget file
```

To regenerate the platform icon set (`.icns`, `.ico`) from the source artwork:

```bash
pnpm tauri icon crates/heretic-app/icons/icon-1024.png
```

<br>

## Setting it up

1. **Connect Flux.** Settings → server URL and API key.
2. **Give a project a folder.** Choose the git repository it lives in.
3. **Assign your models.** Models & roles — a starter configuration uses Claude
   Code for planning and review, and a local Qwen through Codex for the rest.
4. **Switch Auto on** for an epic, either here or in Flux itself.

### Finding what you can run

**Models & roles → What's available** scans for:

- **Agent CLIs** on this machine — Claude Code, Codex and OpenCode, with their
  versions. Anything missing says so, and why.
- **Model hosts** — every configured machine is asked what weights it is
  holding. Ollama is read through its native API, so parameter counts,
  quantisation and sizes come through; anything OpenAI-shaped (vLLM, LM Studio,
  llama.cpp, NIM) is read from `/v1/models`.

Anything found becomes a profile in one click. Where both Codex and OpenCode
are installed, a discovered model offers one button each — the same weights,
driven by either harness. Anything not found can still be added by hand —
detection is a convenience, not a gate.

### Using another machine's models

Point Heretic at any box on your network — a DGX Spark, a workstation with a
GPU, a server in the rack. **Add a host**, give it a name and an address, and its
models appear alongside the local ones.

```
Name:     DGX Spark
Address:  http://spark.local:11434
```

The address is checked before it is saved, so a typo is caught there rather than
at run time. Paste it with or without a trailing `/v1` — both work.

A remote Ollama only answers other machines when it is bound beyond loopback:

```bash
# on the host serving the models
OLLAMA_HOST=0.0.0.0 ollama serve
```

...and the port has to be open through its firewall. Hosts are probed
concurrently, so one machine that is asleep does not hold up the rest.

### Running a local model

Local models need a coding harness to actually edit files. Heretic drives them
through Codex's open-model mode or through OpenCode, whichever you have:

```bash
ollama pull qwen3-coder:30b
```

Adding a model from the scan sets this up for you — press the button for the
harness you want. By hand: set a profile's runner to **Codex — local model
(Ollama)** or **OpenCode**, and its model to `qwen3-coder:30b`. Codex defaults
to `http://localhost:11434/v1`; OpenCode with no endpoint uses the providers
it is already configured with, so give it one to reach a host.

#### Through Codex

**Codex needs Ollama 0.13.4 or newer.** Anything older is refused outright, so
the scan checks each host's version and says so before you hit it mid-run.

Two messages are normal when driving a model Codex does not ship a catalogue
entry for, and neither stops the run:

- `Model metadata for <model> not found. Defaulting to fallback metadata.`
  Heretic passes the real context window when the host reports one, which is
  what the fallback would otherwise guess at.
- `codex_models_manager: failed to refresh available models: missing field
  models`. Codex reads the OpenAI-shaped `/v1/models` listing with its Ollama
  decoder. It is a catalogue refresh, not inference, and is internal to Codex.

Repeated lines are folded together in the run feed, and a backend's own logging
is shown dimmed, so neither drowns the agent's actual output.

Under the hand, a local model runs as:

```bash
codex exec --json --sandbox workspace-write --oss --local-provider ollama -m <model> "<brief>"
```

A model on another machine cannot use `--oss`, because Codex will not let
configuration override its built-in provider ids. Heretic declares one of its
own instead:

```bash
codex exec --json --sandbox workspace-write \
  -c model_providers.heretic-oss.base_url="http://spark.local:11434/v1" \
  -c model_providers.heretic-oss.wire_api="responses" \
  -c model_provider="heretic-oss" \
  -m <model> "<brief>"
```

Current Codex accepts only the `responses` wire format from a custom provider,
which recent Ollama serves at `/v1/responses`.

#### Through OpenCode

OpenCode reads its providers from a configuration file and takes no endpoint
flag, so Heretic writes the provider it needs into `OPENCODE_CONFIG_CONTENT`
for the run. Your own `opencode.json` is never touched, and a profile with no
endpoint is left to use it as-is.

```bash
OPENCODE_CONFIG_CONTENT='{"provider":{"heretic-host":{
  "npm":"@ai-sdk/openai-compatible",
  "options":{"baseURL":"http://spark.local:11434/v1","apiKey":"heretic"},
  "models":{"<model>":{"limit":{"context":262144,"output":32768}}}}}}' \
opencode run --format json --auto -m heretic-host/<model> "<brief>"
```

The model keeps its own id inside the `heretic-host/` prefix, so a
slash-bearing id such as `Qwen/Qwen3-Coder-30B` works. The `limit` is the
context window the host reported, passed on rather than guessed at; OpenCode
wants an output ceiling alongside it and reserves that much of the window for
the reply. The API key is a placeholder — local servers ignore it, but the
OpenAI-compatible provider will not start without one.

Against OpenCode's own providers there is no generated configuration, and the
model is written the way OpenCode writes it — `provider/model`. `opencode
models` lists what yours are set up with.

```bash
opencode run --format json --auto -m anthropic/claude-opus-5 "<brief>"
```

Unlike Codex, OpenCode prices each step as it goes, so a run's spend is summed
from its steps rather than read off a closing total.

### Custom agent CLIs

Any command works. `{{prompt}}` is replaced with the generated brief and
`{{model}}` with the profile's model id; without `{{prompt}}` the brief is sent
on stdin instead.

```
command:  aider
args:     --model {{model}} --message {{prompt}}
```

<br>

## Behind an identity proxy

A Flux server published on a domain usually sits behind something like
Cloudflare Access, oauth2-proxy, Authelia, Pomerium or Tailscale. That proxy
authenticates the request **before** Flux ever sees it, so there are two
credentials in play: the proxy's, and then Flux's own API key.

Settings → **Access** covers both. Whatever you configure is used on every
request, including the live event stream.

### Which option to pick

| Your proxy | Choose | Notes |
|---|---|---|
| Cloudflare Access | Service token | Two headers, no clash with Flux's key. Does not expire. |
| oauth2-proxy, Pomerium | Bearer token | Uses `Authorization` — see the clash below. |
| Authelia, Traefik forward-auth | Sign in, or Custom headers | Cookie-based; sign-in captures it for you. |
| Tailscale / VPN only | Nothing in front of Flux | The network is the boundary; no credential needed. |
| Anything else | Custom headers | Arbitrary header name/value pairs. |

**Prefer a service token for unattended work.** Signing in is convenient, but a
browser session expires — and a run that starts at 2am is not there to sign in
again. Service tokens do not expire.

### Signing in

**Access → Sign in** opens a real browser window at your Flux URL. Complete your
provider's flow as normal; Heretic watches for the resulting session cookie,
verifies it can reach the Flux API with it, then closes the window and keeps it.

### The `Authorization` clash

Flux reads its own API key from `Authorization: Bearer …`. If your proxy wants
that same header, only one credential fits. Heretic gives the header to the
proxy and warns you, because the alternative — silently dropping your proxy
credential — would just look like an outage.

The fix is to let the proxy be the security boundary:

```bash
# Flux, reachable only through the proxy
FLUX_ALLOW_ANONYMOUS=1 flux serve
```

Then leave the API key empty in Heretic. Only do this when Flux is genuinely
unreachable except through the proxy — bound to loopback or an internal network,
never published directly.

Proxies that use their own header (Cloudflare Access) or a cookie have no such
problem: keep your Flux API key as well, and both layers stay authenticated.

### Check what is actually in front of your server

Worth confirming before configuring anything, because a Flux server on a public
domain is not necessarily behind a proxy at all:

```bash
curl -si https://your-flux-host/api/auth/status
```

- A JSON body such as `{"authenticated":false,"authRequired":true}` means you are
  talking to **Flux directly** — no proxy is intercepting the API. Choose
  "Nothing in front of Flux" and set a Flux API key.
- An HTML sign-in page, or a redirect to an identity provider, means a proxy
  **is** in the way. Configure a credential above.

Note that a CDN in front of your server (a `server: cloudflare` header, say) is
not the same thing as Cloudflare **Access**. Only the latter authenticates
requests.

### When it goes wrong

A proxy that rejects you answers with its own sign-in page, not a Flux error —
and because HTTP clients follow redirects, that arrives as a perfectly ordinary
`200` full of HTML. Heretic detects this and says so, naming the provider
where it can, rather than reporting an unintelligible parse failure. The status
in Settings distinguishes *blocked by the proxy* from *Flux rejected the key*,
because the fixes are different.

Heretic also catches a subtler case. A Flux server that requires a key still
answers a keyless `GET /api/projects` with `200` and the *public* projects — an
empty list when your board is private. That looks exactly like a healthy
connection with no work on it, so Heretic checks `/api/auth/status` as well
and tells you a key is needed rather than showing you an empty board.

Use HTTPS. Heretic warns if you send proxy credentials over plain `http://`
to anything other than localhost.

<br>

## Layout

```
crates/heretic-core/   the engine — no UI framework anywhere in it
  flux/                REST client and the SSE watcher
  selection.rs         what may run unattended, and why something may not
  runner/              per-backend argv, process supervision, output parsing
  worktree.rs          git worktrees, diffs, commits, merges
  prompt.rs            the brief each role works from
  orchestrator/        the run state machine and the engine
  history.rs           run journals: what survives a restart
  detect.rs            finding agent CLIs and the models each host holds
crates/heretic-app/    the Tauri shell: commands and events, and little else
ui/                    React + Tailwind front end
```

The engine is deliberately free of Tauri so the orchestration logic can be
tested without launching a desktop app:

```bash
cargo test            # engine, including real subprocess and git behaviour
pnpm typecheck        # interface
```

There is also a suite that runs against a live Flux server behind a proxy. It is
ignored by default since it needs both running:

```bash
FLUX_PROXY_URL=http://localhost:8080 FLUX_API_KEY=flx_… \
  cargo test --test proxy_access -- --ignored
```

The interface also runs in a plain browser against a mock engine, which is
useful for working on it without agents or a server:

```bash
pnpm dev              # http://localhost:5183
```

<br>

## Design notes

**Heretic owns the board, not the agents.** Status changes and comments are
written by the engine, so agents need no Flux access and no credentials. A local
model with no MCP support behaves exactly like Claude Code.

**Guardrails lead.** They are stated before the work, ordered most-critical
first, rather than buried under the task description. A reviewer treats a
guardrail breach as an automatic rejection.

**Autonomy is opt-in and narrow.** The `auto` flag lives on the epic in Flux, so
the decision about what may run unattended stays on the board where the rest of
your team can see it.

**Your checkout is never at risk.** Agents work in their own worktrees, and a
merge back into the base branch is refused outright while the main checkout has
uncommitted changes.

<br>

## Releases

Versions are [semantic](https://semver.org) and cut automatically. Every merge
to `main` is read by
[semantic-release](https://semantic-release.gitbook.io): a `fix:` commit becomes
a patch, a `feat:` a minor, anything breaking a major. The version is written
into `package.json`, `Cargo.toml` and `tauri.conf.json`, tagged, and the desktop
bundles are built and attached to the GitHub release with a `checksums.txt`.

Nothing is released by hand, so the commit message is the release note. See
[CONTRIBUTING.md](CONTRIBUTING.md) for the prefixes, and
[CHANGELOG.md](CHANGELOG.md) for what has shipped.

<br>

## Contributing

Issues and pull requests are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md)
for how to get set up and what CI expects. Everyone taking part is expected to
follow the [code of conduct](CODE_OF_CONDUCT.md).

Found a security problem? Please report it privately: see
[SECURITY.md](SECURITY.md).

<br>

## Related

- [Flux](https://github.com/sirsjg/flux) — the board this is built on
- [Momentum](https://github.com/sirsjg/momentum) — the terminal predecessor,
  which runs a single Claude Code agent per task. Heretic replaces it.

<br>

## Licence

[MIT](LICENSE) © Steve Grehan
