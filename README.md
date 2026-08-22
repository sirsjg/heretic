# Accelerate

A desktop companion for [Flux](https://github.com/sirsjg/flux). Point it at a
project, switch **Auto** on for an epic, and it works the ready tasks with a team
of AI agents — one to plan, one to implement, one to review, one to document.

Each role is bound to a model you choose, so the expensive judgement can sit with
a strong hosted model while the implementation grind runs on a local one.

> Experimental, and built for macOS and Linux.

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
- **Writes back what happened.** Status transitions and a summary comment go to
  Flux under the agent's name, so the board stays honest whether you are
  watching or not.

<br>

## How a run works

```
   Flux board                  Accelerate                     Your repo
┌───────────────┐         ┌──────────────────┐          ┌──────────────────┐
│ epic: auto ✓  │────────▶│ 1. is it ready?  │          │                  │
│ task: todo    │         │ 2. worktree      │─────────▶│ accelerate/<task>│
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
  - or any other agent CLI, described as a custom command
- **git**, and a repository for each project you want worked.

<br>

## Build and run

```bash
pnpm install
pnpm app          # development, with hot reload
pnpm app:build    # a bundled .app / .dmg / .deb / .AppImage
```

On Linux you also need the usual Tauri system packages:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev build-essential curl file
```

To regenerate the platform icon set (`.icns`, `.ico`) from the source artwork:

```bash
pnpm tauri icon crates/accel-app/icons/icon-1024.png
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

- **Agent CLIs** on this machine — Claude Code and Codex, with their versions.
  Anything missing says so, and why.
- **Model hosts** — every configured machine is asked what weights it is
  holding. Ollama is read through its native API, so parameter counts,
  quantisation and sizes come through; anything OpenAI-shaped (vLLM, LM Studio,
  llama.cpp, NIM) is read from `/v1/models`.

Anything found becomes a profile in one click. Anything not found can still be
added by hand — detection is a convenience, not a gate.

### Using another machine's models

Point Accelerate at any box on your network — a DGX Spark, a workstation with a
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

Local models need a coding harness to actually edit files; Accelerate drives
them through Codex's open-model mode:

```bash
ollama pull qwen3-coder:30b
```

Adding a model from the scan sets this up for you. By hand: set a profile's
runner to **Codex — local model (Ollama)** and its model to `qwen3-coder:30b`.
The endpoint defaults to `http://localhost:11434/v1`.

**Codex needs Ollama 0.13.4 or newer.** Anything older is refused outright, so
the scan checks each host's version and says so before you hit it mid-run.

Two messages are normal when driving a model Codex does not ship a catalogue
entry for, and neither stops the run:

- `Model metadata for <model> not found. Defaulting to fallback metadata.`
  Accelerate passes the real context window when the host reports one, which is
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
configuration override its built-in provider ids. Accelerate declares one of its
own instead:

```bash
codex exec --json --sandbox workspace-write \
  -c model_providers.accelerate-oss.base_url="http://spark.local:11434/v1" \
  -c model_providers.accelerate-oss.wire_api="responses" \
  -c model_provider="accelerate-oss" \
  -m <model> "<brief>"
```

Current Codex accepts only the `responses` wire format from a custom provider,
which recent Ollama serves at `/v1/responses`.

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
provider's flow as normal; Accelerate watches for the resulting session cookie,
verifies it can reach the Flux API with it, then closes the window and keeps it.

### The `Authorization` clash

Flux reads its own API key from `Authorization: Bearer …`. If your proxy wants
that same header, only one credential fits. Accelerate gives the header to the
proxy and warns you, because the alternative — silently dropping your proxy
credential — would just look like an outage.

The fix is to let the proxy be the security boundary:

```bash
# Flux, reachable only through the proxy
FLUX_ALLOW_ANONYMOUS=1 flux serve
```

Then leave the API key empty in Accelerate. Only do this when Flux is genuinely
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
`200` full of HTML. Accelerate detects this and says so, naming the provider
where it can, rather than reporting an unintelligible parse failure. The status
in Settings distinguishes *blocked by the proxy* from *Flux rejected the key*,
because the fixes are different.

Accelerate also catches a subtler case. A Flux server that requires a key still
answers a keyless `GET /api/projects` with `200` and the *public* projects — an
empty list when your board is private. That looks exactly like a healthy
connection with no work on it, so Accelerate checks `/api/auth/status` as well
and tells you a key is needed rather than showing you an empty board.

Use HTTPS. Accelerate warns if you send proxy credentials over plain `http://`
to anything other than localhost.

<br>

## Layout

```
crates/accel-core/     the engine — no UI framework anywhere in it
  flux/                REST client and the SSE watcher
  selection.rs         what may run unattended, and why something may not
  runner/              per-backend argv, process supervision, output parsing
  worktree.rs          git worktrees, diffs, commits, merges
  prompt.rs            the brief each role works from
  orchestrator/        the run state machine and the engine
  detect.rs            finding agent CLIs and the models each host holds
crates/accel-app/      the Tauri shell: commands and events, and little else
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

**Accelerate owns the board, not the agents.** Status changes and comments are
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

## Related

- [Flux](https://github.com/sirsjg/flux) — the board this is built on
- [Momentum](https://github.com/sirsjg/momentum) — the terminal companion, which
  runs a single Claude Code agent per task

<br>

## Licence

MIT
