/** Application state: what's loaded, what's selected, and what's running. */

import { create } from "zustand";
import { api, onEngineEvent, onFluxEvent } from "./bridge";
import type {
  BoardView,
  ConnectionState,
  EngineEvent,
  Project,
  ProjectBinding,
  RunRecord,
  Settings,
  SourceKind,
} from "./types";

export type Screen = "board" | "run" | "models" | "settings";

interface Toast {
  id: number;
  level: "info" | "error";
  message: string;
}

interface State {
  ready: boolean;
  screen: Screen;
  projects: Project[];
  selectedProjectId: string | null;
  board: BoardView | null;
  boardLoading: boolean;
  settings: Settings | null;
  runs: RunRecord[];
  selectedRunId: string | null;
  connection: ConnectionState;
  toasts: Toast[];
  theme: "dark" | "light";
  /** True while a Flux re-sync is in flight, so a refresh button can show it. */
  syncing: boolean;

  initialise: () => Promise<void>;
  selectProject: (projectId: string) => Promise<void>;
  refreshBoard: () => Promise<void>;
  /** Quietly re-read projects and the open board after Flux changes. */
  syncFromFlux: () => Promise<void>;
  openScreen: (screen: Screen) => void;
  openRun: (runId: string) => void;
  setEpicAuto: (epicId: string, auto: boolean) => Promise<void>;
  startTask: (taskId: string) => Promise<void>;
  runReady: () => Promise<void>;
  stopRun: (runId: string) => Promise<void>;
  dismissRun: (runId: string) => Promise<void>;
  integrateRun: (runId: string) => Promise<void>;
  discardRunWork: (runId: string) => Promise<void>;
  saveSettings: (settings: Settings) => Promise<void>;
  saveBinding: (binding: ProjectBinding) => Promise<void>;
  testConnection: () => Promise<void>;
  /** Re-read everything after the connection changes (sign-in, new key, new URL). */
  reconnect: () => Promise<void>;
  toggleTheme: () => void;
  notify: (level: "info" | "error", message: string) => void;
  dismissToast: (id: number) => void;
}

let toastId = 0;

/// Engine events are subscribed once for the process. React re-mounts the app
/// (StrictMode does it deliberately in development), and a second subscription
/// would apply every event twice — which shows up as a duplicated feed.
let unsubscribeEngine: (() => void) | null = null;
let unsubscribeFlux: (() => void) | null = null;

/// Flux announces every mutation separately; one drag on its board can be a
/// burst of events. Collapse a burst into a single re-fetch.
let fluxSyncTimer: ReturnType<typeof setTimeout> | null = null;

/** Which tracker a project was listed from, for routing commands back to it. */
function projectSource(state: State, projectId: string | null): SourceKind | undefined {
  if (!projectId) return undefined;
  return state.projects.find((project) => project.id === projectId)?.source;
}

/** The binding for the selected project, if it has one. */
export function currentBinding(state: State): ProjectBinding | null {
  if (!state.settings || !state.selectedProjectId) return null;
  return (
    state.settings.bindings.find(
      (b) => b.project_id === state.selectedProjectId,
    ) ?? null
  );
}

export const useStore = create<State>((set, get) => ({
  ready: false,
  screen: "board",
  projects: [],
  selectedProjectId: null,
  board: null,
  boardLoading: false,
  settings: null,
  runs: [],
  selectedRunId: null,
  connection: { connected: false },
  toasts: [],
  syncing: false,
  theme:
    (typeof localStorage !== "undefined" &&
      (localStorage.getItem("heretic.theme") as "dark" | "light" | null)) ||
    "dark",

  async initialise() {
    applyTheme(get().theme);

    // macOS puts its window controls over our sidebar, so the layout has to
    // know which platform it is on before first paint.
    const os = await api.platform().catch(() => "browser");
    document.documentElement.setAttribute("data-os", os);

    try {
      const [settings, projects, runs, connection] = await Promise.all([
        api.getSettings(),
        api.listProjects().catch(() => [] as Project[]),
        api.listRuns().catch(() => [] as RunRecord[]),
        api.testConnection().catch(() => ({ connected: false }) as ConnectionState),
      ]);

      set({ settings, projects, runs, connection, ready: true });

      // Prefer a project that is actually set up, so the app opens on
      // something workable rather than the first one alphabetically.
      const bound = projects.find((project) =>
        settings.bindings.some((binding) => binding.project_id === project.id),
      );
      const opening = bound?.id ?? projects[0]?.id ?? null;
      if (opening) await get().selectProject(opening);
    } catch (error) {
      set({ ready: true });
      get().notify("error", describe(error));
    }

    unsubscribeEngine?.();
    unsubscribeEngine = onEngineEvent((event) =>
      applyEngineEvent(event, set, get),
    );

    // Anything changing on the Flux server — a project created or deleted, a
    // task edited in its web UI — lands here and re-syncs the sidebar and board.
    unsubscribeFlux?.();
    unsubscribeFlux = onFluxEvent((event) => {
      if (event.kind === "disconnected") return;
      if (fluxSyncTimer) return;
      fluxSyncTimer = setTimeout(() => {
        fluxSyncTimer = null;
        void get().syncFromFlux();
      }, 400);
    });
  },

  async selectProject(projectId) {
    set({ selectedProjectId: projectId, screen: "board" });
    await get().refreshBoard();
  },

  async refreshBoard() {
    const projectId = get().selectedProjectId;
    if (!projectId) return;
    set({ boardLoading: true });
    try {
      const board = await api.board(projectId, projectSource(get(), projectId));
      set({ board, boardLoading: false });
    } catch (error) {
      set({ boardLoading: false });
      get().notify("error", describe(error));
    }
  },

  async syncFromFlux() {
    // Silent on purpose: this runs on every remote change, so it must not
    // flash spinners over the board or toast when the server is briefly away.
    set({ syncing: true });
    try {
      const projects = await api.listProjects();
      set({ projects });

      const current = get().selectedProjectId;
      if (current && projects.some((project) => project.id === current)) {
        const board = await api.board(current, projectSource(get(), current));
        // Only apply if the user hasn't switched projects in the meantime.
        if (get().selectedProjectId === current) set({ board });
        return;
      }

      // The open project is gone (or nothing was open) — fall back the same
      // way startup does: a bound project first, then whatever exists.
      const bound = projects.find((project) =>
        (get().settings?.bindings ?? []).some(
          (binding) => binding.project_id === project.id,
        ),
      );
      const opening = bound?.id ?? projects[0]?.id ?? null;
      if (opening) await get().selectProject(opening);
      else set({ selectedProjectId: null, board: null });
    } catch {
      // Offline or mid-reconnect; the next event will try again.
    } finally {
      set({ syncing: false });
    }
  },

  openScreen(screen) {
    set({ screen });
  },

  openRun(runId) {
    set({ selectedRunId: runId, screen: "run" });
  },

  async setEpicAuto(epicId, auto) {
    // Optimistic: the switch should feel instant.
    const board = get().board;
    if (board) {
      set({
        board: {
          ...board,
          epics: board.epics.map((e) => (e.id === epicId ? { ...e, auto } : e)),
        },
      });
    }
    const source = get().board?.project.source;
    try {
      await api.setEpicAuto(epicId, auto, source);
      // A Linear epic's flag lives in Heretic's settings, which the backend
      // just rewrote — re-read them, or the next full settings save would
      // push a stale copy and silently undo the toggle.
      if (source === "linear") set({ settings: await api.getSettings() });
      await get().refreshBoard();
    } catch (error) {
      get().notify("error", describe(error));
      await get().refreshBoard();
    }
  },

  async startTask(taskId) {
    const projectId = get().selectedProjectId;
    if (!projectId) return;
    try {
      const runId = await api.startTask(projectId, taskId);
      set({ selectedRunId: runId, screen: "run" });
      await get().refreshBoard();
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async runReady() {
    try {
      const started = await api.runReady();
      if (started.length === 0) {
        get().notify("info", "Nothing is ready to run right now.");
        return;
      }
      set({ selectedRunId: started[0]!, screen: "run" });
      await get().refreshBoard();
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async stopRun(runId) {
    await api.stopRun(runId);
    set({ runs: await api.listRuns() });
  },

  async dismissRun(runId) {
    await api.dismissRun(runId);
    const runs = await api.listRuns();
    set({
      runs,
      selectedRunId: get().selectedRunId === runId ? null : get().selectedRunId,
      screen: get().selectedRunId === runId ? "board" : get().screen,
    });
  },

  async integrateRun(runId) {
    try {
      await api.integrateRun(runId);
      set({ runs: await api.listRuns() });
      get().notify("info", "Merged, and the worktree is gone.");
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async discardRunWork(runId) {
    try {
      await api.discardRunWork(runId);
      set({ runs: await api.listRuns() });
      get().notify("info", "Work discarded.");
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async saveSettings(settings) {
    try {
      await api.saveSettings(settings);
      set({ settings });
      await get().testConnection();
      get().notify("info", "Settings saved.");
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async saveBinding(binding) {
    try {
      await api.saveBinding(binding);
      set({ settings: await api.getSettings() });
      await get().refreshBoard();
    } catch (error) {
      get().notify("error", describe(error));
    }
  },

  async reconnect() {
    // Settings may have been changed by the Rust side (signing in stores a
    // cookie), and a project list fetched while blocked is empty — so both have
    // to be re-read, not just the connection status.
    const settings = await api.getSettings().catch(() => get().settings);
    if (settings) set({ settings });

    await get().testConnection();

    const projects = await api.listProjects().catch(() => [] as Project[]);
    set({ projects });

    const current = get().selectedProjectId;
    const stillThere = projects.some((project) => project.id === current);
    if (stillThere) {
      await get().refreshBoard();
      return;
    }

    const bound = projects.find((project) =>
      (settings?.bindings ?? []).some((b) => b.project_id === project.id),
    );
    const opening = bound?.id ?? projects[0]?.id ?? null;
    if (opening) await get().selectProject(opening);
    else set({ selectedProjectId: null, board: null });
  },

  async testConnection() {
    const connection = await api
      .testConnection()
      .catch((error): ConnectionState => ({ connected: false, error: describe(error) }));
    set({ connection });
  },

  toggleTheme() {
    const theme = get().theme === "dark" ? "light" : "dark";
    applyTheme(theme);
    localStorage.setItem("heretic.theme", theme);
    set({ theme });
  },

  notify(level, message) {
    const id = ++toastId;
    set({ toasts: [...get().toasts, { id, level, message }] });
    setTimeout(() => get().dismissToast(id), 6000);
  },

  dismissToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) });
  },
}));

function applyEngineEvent(
  event: EngineEvent,
  set: (partial: Partial<State>) => void,
  get: () => State,
) {
  switch (event.kind) {
    case "run_updated": {
      const runs = get().runs.slice();
      const index = runs.findIndex((r) => r.id === event.run.id);
      // Once a run is known, its feed is owned by the run_output stream. Taking
      // the summary's copy again would re-add items already applied, which is
      // how a feed ends up showing every line twice.
      const existing = index >= 0 ? runs[index] : undefined;
      const merged = { ...event.run, feed: existing ? existing.feed : event.run.feed };
      if (index >= 0) runs[index] = merged;
      else runs.unshift(merged);
      set({ runs });

      if (!get().selectedRunId) set({ selectedRunId: event.run.id });
      if (event.run.status !== "running" && event.run.status !== "queued") {
        void get().refreshBoard();
      }
      break;
    }

    case "run_output": {
      const runs = get().runs.map((run) =>
        run.id === event.run_id
          ? { ...run, feed: [...run.feed, event.item].slice(-500) }
          : run,
      );
      set({ runs });
      break;
    }

    case "notice":
      get().notify(event.level === "error" ? "error" : "info", event.message);
      break;
  }
}

function applyTheme(theme: "dark" | "light") {
  if (typeof document !== "undefined") {
    document.documentElement.setAttribute("data-theme", theme);
  }
}

function describe(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return String(error);
}
