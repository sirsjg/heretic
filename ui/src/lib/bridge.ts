/**
 * The single seam between the interface and the engine.
 *
 * Inside the desktop shell every call goes to Rust over Tauri's IPC. In a plain
 * browser it falls back to the mock engine, so the UI can be developed and
 * reviewed without a Flux server, agents, or a repository.
 */

import type {
  BoardView,
  ConnectionState,
  EngineEvent,
  Project,
  ProjectBinding,
  RunRecord,
  Settings,
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

  async board(projectId: string): Promise<BoardView> {
    return isDesktop()
      ? invoke("get_board", { projectId })
      : mock.board(projectId);
  },

  async setEpicAuto(epicId: string, auto: boolean): Promise<void> {
    if (isDesktop()) return invoke("set_epic_auto", { epicId, auto });
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

  let unlisten: (() => void) | undefined;
  let cancelled = false;

  void (async () => {
    const { listen } = await import("@tauri-apps/api/event");
    const stop = await listen<EngineEvent>("engine://event", (event) =>
      handler(event.payload),
    );
    if (cancelled) stop();
    else unlisten = stop;
  })();

  return () => {
    cancelled = true;
    unlisten?.();
  };
}
