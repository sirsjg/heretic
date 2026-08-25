/**
 * The single seam between the interface and the engine.
 *
 * Inside the desktop shell every call goes to Rust over Tauri's IPC. In a plain
 * browser it falls back to the mock engine, so the UI can be developed and
 * reviewed without a Flux server, agents, or a repository.
 */

import type {
  BoardView,
  Environment,
  FileChange,
  HostProbe,
  ModelHost,
  ConnectionState,
  EngineEvent,
  FluxEvent,
  Project,
  ProjectBinding,
  RunCommit,
  RunRecord,
  Settings,
  SourceKind,
} from "./types";
import { MockEngine } from "./mock";

/** True when running inside the Tauri shell. */
export const isDesktop = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

const mock = new MockEngine();

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

export const api = {
  async listProjects(): Promise<Project[]> {
    return isDesktop() ? invoke("list_projects") : mock.listProjects();
  },

  async board(projectId: string, source?: SourceKind): Promise<BoardView> {
    return isDesktop()
      ? invoke("get_board", { projectId, source })
      : mock.board(projectId);
  },

  async setEpicAuto(
    epicId: string,
    auto: boolean,
    source?: SourceKind,
  ): Promise<void> {
    if (isDesktop()) return invoke("set_epic_auto", { epicId, auto, source });
    mock.setEpicAuto(epicId, auto);
  },

  async getSettings(): Promise<Settings> {
    return isDesktop() ? invoke("get_settings") : mock.getSettings();
  },

  async saveSettings(settings: Settings): Promise<void> {
    if (isDesktop()) return invoke("save_settings", { settings });
    mock.saveSettings(settings);
  },

  async saveBinding(binding: ProjectBinding): Promise<void> {
    if (isDesktop()) return invoke("save_binding", { binding });
    const settings = mock.getSettings();
    const index = settings.bindings.findIndex(
      (b) => b.project_id === binding.project_id,
    );
    if (index >= 0) settings.bindings[index] = binding;
    else settings.bindings.push(binding);
    mock.saveSettings(settings);
  },

  async testConnection(): Promise<ConnectionState> {
    return isDesktop()
      ? invoke("test_connection")
      : { connected: true, error: null };
  },

  /** One authenticated whoami round trip against Linear. */
  async testLinearConnection(): Promise<ConnectionState> {
    return isDesktop()
      ? invoke("test_linear_connection")
      : { connected: true, error: null };
  },

  /** Open a folder picker and return the chosen path, or null if dismissed. */
  async chooseFolder(): Promise<string | null> {
    if (!isDesktop()) {
      return window.prompt("Repository path", "/Users/you/code/project");
    }
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({ directory: true, multiple: false });
    return typeof selected === "string" ? selected : null;
  },

  async revealPath(path: string): Promise<void> {
    if (!isDesktop()) return;
    const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
    await revealItemInDir(path);
  },

  /**
   * Open a window at the Flux server so the user can complete their identity
   * provider's flow; the resulting session cookie is kept.
   */
  async signIn(): Promise<string> {
    if (!isDesktop()) return "Sign-in needs the desktop app.";
    return invoke("flux_sign_in");
  },

  /** "macos" | "linux" | ... — used to leave room for window controls. */
  async platform(): Promise<string> {
    if (!isDesktop()) return "browser";
    return invoke("platform");
  },

  async signOut(): Promise<void> {
    if (!isDesktop()) return;
    return invoke("flux_sign_out");
  },

  /** Scan for agent CLIs and for the models each configured host is holding. */
  async detectEnvironment(): Promise<Environment> {
    if (isDesktop()) return invoke("detect_environment");
    return {
      clis: [
        { program: "claude", label: "Claude Code", found: true, version: "2.0.44" },
        {
          program: "codex",
          label: "Codex",
          found: false,
          problem: "`codex` was not found on PATH.",
        },
      ],
      hosts: [
        {
          host: { id: "local-ollama", name: "This machine", base_url: "http://localhost:11434" },
          reachable: true,
          kind: "ollama",
          models: [
            { id: "qwen3-coder:30b", parameter_size: "30.5B", quantization: "Q4_K_M", size_bytes: 18500000000 },
            { id: "llama3.1:8b", parameter_size: "8.0B", quantization: "Q4_0", size_bytes: 4700000000 },
          ],
        },
        {
          host: { id: "spark", name: "DGX Spark", base_url: "http://spark.local:11434" },
          reachable: true,
          kind: "ollama",
          models: [
            { id: "qwen3-coder:480b", parameter_size: "480B", quantization: "Q4_K_M", size_bytes: 270000000000 },
            { id: "deepseek-r1:70b", parameter_size: "70B", quantization: "Q8_0", size_bytes: 74000000000 },
          ],
        },
      ],
      os: "browser",
    };
  },

  /** Look at an address without saving it. */
  async probeHost(name: string, baseUrl: string): Promise<HostProbe> {
    if (isDesktop()) return invoke("probe_host", { name, baseUrl });
    return {
      host: { id: "probe", name, base_url: baseUrl },
      reachable: true,
      kind: "ollama",
      models: [{ id: "qwen3-coder:30b", parameter_size: "30.5B" }],
    };
  },

  async saveHost(host: ModelHost): Promise<void> {
    if (isDesktop()) return invoke("save_host", { host });
  },

  async removeHost(hostId: string): Promise<void> {
    if (isDesktop()) return invoke("remove_host", { hostId });
  },

  /** The OpenAI-compatible base a runner should use for a host. */
  async openaiBase(baseUrl: string): Promise<string> {
    if (isDesktop()) return invoke("openai_base", { baseUrl });
    return `${baseUrl.replace(/\/+$/, "").replace(/\/v1$/, "")}/v1`;
  },

  async listRuns(): Promise<RunRecord[]> {
    return isDesktop() ? invoke("list_runs") : mock.listRuns();
  },

  async startTask(projectId: string, taskId: string): Promise<string> {
    return isDesktop()
      ? invoke("start_task", { projectId, taskId })
      : mock.startTask(projectId, taskId);
  },

  async stopRun(runId: string): Promise<boolean> {
    return isDesktop() ? invoke("stop_run", { runId }) : mock.stopRun(runId);
  },

  async dismissRun(runId: string): Promise<boolean> {
    return isDesktop()
      ? invoke("dismiss_run", { runId })
      : mock.dismissRun(runId);
  },

  /** Merge a finished run's branch back, and remove its worktree. */
  async integrateRun(runId: string): Promise<void> {
    if (isDesktop()) return invoke("integrate_run", { runId });
  },

  /** Throw a finished run's work away. */
  async discardRunWork(runId: string): Promise<void> {
    if (isDesktop()) return invoke("discard_run_work", { runId });
  },

  /** Every file a run touched, with its line counts. */
  async runChangedFiles(runId: string): Promise<FileChange[]> {
    return isDesktop()
      ? invoke("run_changed_files", { runId })
      : mock.runChangedFiles(runId);
  },

  /** One file's diff, as a unified patch. */
  async runFileDiff(runId: string, path: string): Promise<string> {
    return isDesktop()
      ? invoke("run_file_diff", { runId, path })
      : mock.runFileDiff(runId, path);
  },

  /** The commits a run put on its branch, newest first. */
  async runCommits(runId: string): Promise<RunCommit[]> {
    return isDesktop() ? invoke("run_commits", { runId }) : mock.runCommits(runId);
  },

  /** The patch one of those commits introduced. */
  async runCommitDiff(runId: string, sha: string): Promise<string> {
    return isDesktop()
      ? invoke("run_commit_diff", { runId, sha })
      : mock.runCommitDiff(runId, sha);
  },

  /** Start whatever auto-enabled work is ready right now. */
  async runReady(): Promise<string[]> {
    if (isDesktop()) return invoke("tick_auto");
    const board = mock.board(mock.listProjects()[0]!.id);
    const next = board.ready[0];
    return next ? [mock.startTask(board.project.id, next)] : [];
  },
};

/** Subscribe to engine events. Returns an unsubscribe function. */
export function onEngineEvent(
  handler: (event: EngineEvent) => void,
): () => void {
  if (!isDesktop()) return mock.subscribe(handler);
  return listenDesktop("engine://event", handler);
}

/**
 * Subscribe to changes the Flux server announces on its live stream, relayed
 * by the Rust side. In the browser there is no server, so nothing arrives.
 */
export function onFluxEvent(handler: (event: FluxEvent) => void): () => void {
  if (!isDesktop()) return () => {};
  return listenDesktop("flux://event", handler);
}

function listenDesktop<T>(name: string, handler: (event: T) => void): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;

  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const stop = await listen<T>(name, (event) => handler(event.payload));
    if (cancelled) stop();
    else unlisten = stop;
  })();

  return () => {
    cancelled = true;
    unlisten?.();
  };
}
