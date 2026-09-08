/**
 * The single seam between the interface and the engine.
 *
 * There are three ways this interface can be running, and every call below
 * picks its transport from that:
 *
 * - **desktop** — inside the Tauri shell, where every call goes to Rust over
 *   Tauri's IPC.
 * - **remote** — served by `heretic-server` to a browser (a phone, usually),
 *   where the same commands go over HTTP and events arrive on a WebSocket.
 * - **mock** — a plain browser in development, against a scripted engine, so
 *   the UI can be built without a Flux server, agents, or a repository.
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
  RemoteStatus,
  RunCommit,
  RunRecord,
  Settings,
  SourceKind,
} from "./types";
import { MockEngine } from "./mock";
import * as remote from "./remote";

export type Mode = "desktop" | "remote" | "mock";

/** True when running inside the Tauri shell. */
export const isDesktop = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Which transport to use. A built bundle in a browser can only have come from
 * `heretic-server`; the development server is the one place the mock lives.
 */
export function mode(): Mode {
  if (isDesktop()) return "desktop";
  if (import.meta.env.DEV) return "mock";
  return "remote";
}

export const isRemote = (): boolean => mode() === "remote";

const mock = new MockEngine();

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (mode() === "remote") return remote.call<T>(command, args);
  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

/** Whether a call should go to a real engine rather than the mock. */
const live = (): boolean => mode() !== "mock";

export const api = {
  async listProjects(): Promise<Project[]> {
    return live() ? invoke("list_projects") : mock.listProjects();
  },

  async board(projectId: string, source?: SourceKind): Promise<BoardView> {
    return live()
      ? invoke("get_board", { projectId, source })
      : mock.board(projectId);
  },

  /** Flip an epic's Auto switch, on whichever tracker the epic lives. */
  async setEpicAuto(epicId: string, auto: boolean, source?: SourceKind): Promise<void> {
    if (live()) return invoke("set_epic_auto", { epicId, auto, source });
    mock.setEpicAuto(epicId, auto);
  },

  async getSettings(): Promise<Settings> {
    return live() ? invoke("get_settings") : mock.getSettings();
  },

  async saveSettings(settings: Settings): Promise<void> {
    if (live()) return invoke("save_settings", { settings });
    mock.saveSettings(settings);
  },

  async saveBinding(binding: ProjectBinding): Promise<void> {
    if (live()) return invoke("save_binding", { binding });
    const settings = mock.getSettings();
    const index = settings.bindings.findIndex((b) => b.project_id === binding.project_id);
    if (index >= 0) settings.bindings[index] = binding;
    else settings.bindings.push(binding);
    mock.saveSettings(settings);
  },

  async testConnection(): Promise<ConnectionState> {
    return live()
      ? invoke("test_connection")
      : { connected: true, error: null };
  },

  /** Exercise the Linear connection with one authenticated round trip. */
  async testLinearConnection(): Promise<ConnectionState> {
    return live()
      ? invoke("test_linear_connection")
      : { connected: true, error: null };
  },

  /**
   * Open a folder picker and return the chosen path, or null if dismissed.
   *
   * There is no picker for a folder on another machine, so remote asks for
   * the path as text — it is the desktop's filesystem being described.
   */
  async chooseFolder(): Promise<string | null> {
    if (!isDesktop()) {
      return window.prompt(
        isRemote()
          ? "Repository path on the machine running Heretic"
          : "Repository path",
        "/Users/you/code/project",
      );
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

  /**
   * "macos" | "linux" | "remote" | "browser" — used to leave room for window
   * controls, which only the desktop has.
   */
  async platform(): Promise<string> {
    if (isDesktop()) return invoke("platform");
    return isRemote() ? "remote" : "browser";
  },

  async signOut(): Promise<void> {
    if (!live()) return;
    return invoke("flux_sign_out");
  },

  /** Scan for agent CLIs and for the models each configured host is holding. */
  async detectEnvironment(): Promise<Environment> {
    if (live()) return invoke("detect_environment");
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
    if (live()) return invoke("probe_host", { name, baseUrl });
    return {
      host: { id: "probe", name, base_url: baseUrl },
      reachable: true,
      kind: "ollama",
      models: [{ id: "qwen3-coder:30b", parameter_size: "30.5B" }],
    };
  },

  async saveHost(host: ModelHost): Promise<void> {
    if (live()) return invoke("save_host", { host });
  },

  async removeHost(hostId: string): Promise<void> {
    if (live()) return invoke("remove_host", { hostId });
  },

  /** The OpenAI-compatible base a runner should use for a host. */
  async openaiBase(baseUrl: string): Promise<string> {
    if (live()) return invoke("openai_base", { baseUrl });
    return `${baseUrl.replace(/\/+$/, "").replace(/\/v1$/, "")}/v1`;
  },

  async listRuns(): Promise<RunRecord[]> {
    return live() ? invoke("list_runs") : mock.listRuns();
  },

  async startTask(projectId: string, taskId: string): Promise<string> {
    return live()
      ? invoke("start_task", { projectId, taskId })
      : mock.startTask(projectId, taskId);
  },

  async stopRun(runId: string): Promise<boolean> {
    return live() ? invoke("stop_run", { runId }) : mock.stopRun(runId);
  },

  /** Answer the question a paused run is waiting on. */
  async answerQuestion(runId: string, answer: string): Promise<boolean> {
    return live()
      ? invoke("answer_question", { runId, answer })
      : mock.answerQuestion(runId, answer);
  },

  async dismissRun(runId: string): Promise<boolean> {
    return live()
      ? invoke("dismiss_run", { runId })
      : mock.dismissRun(runId);
  },

  /** Merge a finished run's branch back and remove its worktree. */
  async integrateRun(runId: string): Promise<void> {
    if (live()) return invoke("integrate_run", { runId });
  },

  /** Delete a finished run's branch and worktree. */
  async discardRunWork(runId: string): Promise<void> {
    if (live()) return invoke("discard_run_work", { runId });
  },

  /** Every file a run touched, with line counts. */
  async runChangedFiles(runId: string): Promise<FileChange[]> {
    return live()
      ? invoke("run_changed_files", { runId })
      : mock.runChangedFiles(runId);
  },

  /** One file's diff from a run, as a unified patch. */
  async runFileDiff(runId: string, path: string): Promise<string> {
    return live()
      ? invoke("run_file_diff", { runId, path })
      : mock.runFileDiff(runId, path);
  },

  /** The commits a run put on its branch, newest first. */
  async runCommits(runId: string): Promise<RunCommit[]> {
    return live() ? invoke("run_commits", { runId }) : mock.runCommits(runId);
  },

  /** The patch one of a run's commits introduced. */
  async runCommitDiff(runId: string, sha: string): Promise<string> {
    return live()
      ? invoke("run_commit_diff", { runId, sha })
      : mock.runCommitDiff(runId, sha);
  },

  /** Start whatever auto-enabled work is ready now. Returns the run ids started. */
  async runReady(): Promise<string[]> {
    if (live()) return invoke("tick_auto");
    const board = mock.board(mock.listProjects()[0]!.id);
    const next = board.ready[0];
    return next ? [mock.startTask(board.project.id, next)] : [];
  },

  // --- Remote access ---------------------------------------------------------

  /**
   * Whether the desktop's listener is up, and the link a phone pairs with.
   *
   * Only the desktop knows: from a phone the listener is simply what answered.
   */
  async remoteStatus(): Promise<RemoteStatus> {
    if (isDesktop()) return invoke("remote_status");
    if (isRemote()) {
      return {
        enabled: true,
        listening: location.host,
        addresses: [],
        interfaces: [],
        pairing_url: null,
        pairing_qr: null,
      };
    }
    return mock.remoteStatus();
  },

  /** Mint a new remote token, logging every paired device out. */
  async rotateRemoteToken(): Promise<string> {
    if (live()) return invoke("rotate_remote_token");
    return mock.rotateRemoteToken();
  },

  /** Post a test message to the configured notification services. */
  async testNotifications(): Promise<void> {
    if (live()) return invoke("test_notifications");
  },
};

/** Subscribe to engine events. Returns an unsubscribe function. */
export function onEngineEvent(
  handler: (event: EngineEvent) => void,
): () => void {
  switch (mode()) {
    case "mock":
      return mock.subscribe(handler);
    case "remote":
      return remote.onEngineEvent(handler);
    case "desktop":
      return listenDesktop("engine://event", handler);
  }
}

/**
 * Subscribe to changes the Flux server announces on its live stream, relayed
 * by the Rust side. In the mock there is no server, so nothing arrives.
 */
export function onFluxEvent(handler: (event: FluxEvent) => void): () => void {
  switch (mode()) {
    case "mock":
      return () => {};
    case "remote":
      return remote.onFluxEvent(handler);
    case "desktop":
      return listenDesktop("flux://event", handler);
  }
}

/**
 * Whether the link to the engine is up. Only remote can lose it; the desktop
 * and the mock are always connected, so the handler is never called there.
 */
export function onConnectivity(
  handler: (state: remote.Connectivity) => void,
): () => void {
  return isRemote() ? remote.onConnectivity(handler) : () => {};
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
