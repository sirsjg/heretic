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

### Running a local model

Local models need a coding harness to actually edit files; Accelerate drives
them through Codex's open-model mode:

```bash
ollama pull qwen3-coder:30b
```

Then set a profile's runner to **Codex — local model (Ollama)** and its model to
`qwen3-coder:30b`. The endpoint defaults to `http://localhost:11434/v1`.

### Custom agent CLIs

Any command works. `{{prompt}}` is replaced with the generated brief and
`{{model}}` with the profile's model id; without `{{prompt}}` the brief is sent
on stdin instead.

```
command:  aider
args:     --model {{model}} --message {{prompt}}
```

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
crates/accel-app/      the Tauri shell: commands and events, and little else
ui/                    React + Tailwind front end
```

The engine is deliberately free of Tauri so the orchestration logic can be
tested without launching a desktop app:

```bash
cargo test            # engine, including real subprocess and git behaviour
pnpm typecheck        # interface
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
