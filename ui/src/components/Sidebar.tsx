import { useStore } from "../lib/store";
import type { ConnectionState } from "../lib/types";
import { Badge, Dot, cx } from "./ui";
import {
  IconBoard,
  IconModels,
  IconMoon,
  IconSettings,
  IconSparks,
  IconSun,
} from "./icons";

export function Sidebar() {
  const {
    projects,
    selectedProjectId,
    selectProject,
    screen,
    openScreen,
    settings,
    connection,
    runs,
    theme,
    toggleTheme,
  } = useStore();

  const activeRuns = runs.filter(
    (run) => run.status === "running" || run.status === "queued",
  ).length;

  return (
    <aside
      style={{ background: "var(--surface)" }}
      className="flex w-60 shrink-0 flex-col border-r"
    >
      <div
        data-tauri-drag-region="deep"
        className="titlebar-safe flex h-14 items-center gap-2 px-4 pt-1"
      >
        <div
          className="grid size-6 place-items-center rounded-md"
          style={{
            background: "linear-gradient(135deg, var(--accent), var(--accent-2))",
          }}
        >
          <IconSparks className="size-3.5 text-white" />
        </div>
        <span className="text-[14px] font-semibold tracking-tight">Heretic</span>
      </div>

      <ConnectionPill connection={connection} url={settings?.flux.base_url} />

      <nav className="flex-1 overflow-y-auto px-2 pb-2">
        <p className="px-2 pb-1.5 pt-3 text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
          Projects
        </p>

        {projects.length === 0 && (
          <p className="px-2 py-2 text-[12px] leading-snug text-[var(--text-faint)]">
            No projects yet. Connect a Flux server in Settings.
          </p>
        )}

        {projects.map((project) => {
          const binding = settings?.bindings.find(
            (b) => b.project_id === project.id,
          );
          const active = project.id === selectedProjectId && screen === "board";
          return (
            <button
              key={project.id}
              onClick={() => void selectProject(project.id)}
              style={active ? { background: "var(--accent-soft)" } : undefined}
              className={cx(
                "group mb-0.5 flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left transition-colors",
                !active && "hover:bg-[var(--surface-3)]",
              )}
            >
              <span
                className="size-1.5 shrink-0 rounded-full"
                style={{
                  background: binding
                    ? "var(--success)"
                    : "var(--text-faint)",
                }}
                title={binding ? "Folder configured" : "No folder set"}
              />
              <span
                className={cx(
                  "min-w-0 flex-1 truncate text-[13px]",
                  active && "font-medium",
                )}
                style={active ? { color: "var(--accent-text)" } : undefined}
              >
                {project.name}
              </span>
              {binding?.auto_run && (
                <Badge tone="accent" title="Auto-run is on for this project">
                  Auto
                </Badge>
              )}
            </button>
          );
        })}
      </nav>

      <div className="border-t p-2">
        <NavItem
          icon={<IconBoard />}
          label="Runs"
          badge={activeRuns > 0 ? String(activeRuns) : undefined}
          active={screen === "run"}
          onClick={() => openScreen("run")}
        />
        <NavItem
          icon={<IconModels />}
          label="Models & roles"
          active={screen === "models"}
          onClick={() => openScreen("models")}
        />
        <NavItem
          icon={<IconSettings />}
          label="Settings"
          active={screen === "settings"}
          onClick={() => openScreen("settings")}
        />
        <button
          onClick={toggleTheme}
          className="mt-1 flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] text-[var(--text-muted)] transition-colors hover:bg-[var(--surface-3)]"
        >
          {theme === "dark" ? <IconSun /> : <IconMoon />}
          {theme === "dark" ? "Light theme" : "Dark theme"}
        </button>
      </div>
    </aside>
  );
}

function NavItem({
  icon,
  label,
  badge,
  active,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  badge?: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      style={active ? { background: "var(--accent-soft)", color: "var(--accent-text)" } : undefined}
      className={cx(
        "flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-[13px] transition-colors",
        !active && "text-[var(--text-muted)] hover:bg-[var(--surface-3)]",
      )}
    >
      {icon}
      <span className="flex-1 text-left">{label}</span>
      {badge && <Badge tone="accent">{badge}</Badge>}
    </button>
  );
}

function ConnectionPill({
  connection,
  url,
}: {
  connection: ConnectionState;
  url?: string;
}) {
  const host = url?.replace(/^https?:\/\//, "") ?? "not configured";
  // Naming the layer matters: a proxy challenge is fixed under Access, an
  // unreachable server is not.
  const label = connection.connected
    ? "Flux connected"
    : connection.kind === "proxy_challenge"
      ? "Blocked by the proxy"
      : connection.kind === "flux_auth"
        ? "Flux rejected the key"
        : "Flux unreachable";
  return (
    <div className="mx-2 mb-1 flex items-center gap-2 rounded-lg px-2 py-1.5" style={{ background: "var(--surface-2)" }}>
      <Dot tone={connection.connected ? "success" : "danger"} pulse={connection.connected} />
      <div className="min-w-0 flex-1">
        <p className="truncate text-[11.5px] font-medium leading-tight">{label}</p>
        <p className="truncate text-[10.5px] leading-tight text-[var(--text-faint)]" title={connection.error ?? host}>
          {connection.error ?? host}
        </p>
      </div>
    </div>
  );
}
