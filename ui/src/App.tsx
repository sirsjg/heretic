import { useEffect, useState } from "react";
import { useStore } from "./lib/store";
import { isDesktop, isRemote } from "./lib/bridge";
import { Sidebar } from "./components/Sidebar";
import { BoardView } from "./views/BoardView";
import { RunView } from "./views/RunView";
import { ModelsView } from "./views/ModelsView";
import { SettingsView } from "./views/SettingsView";
import { Badge, Button, Input, Spinner, cx } from "./components/ui";
import {
  IconBoard,
  IconClose,
  IconMenu,
  IconModels,
  IconOffline,
  IconSettings,
} from "./components/icons";
import { isActive } from "./lib/types";

export default function App() {
  const { ready, screen, initialise, toasts, dismissToast, paired, online } = useStore();

  useEffect(() => {
    void initialise();
  }, [initialise]);

  if (isRemote() && ready && !paired) {
    return <PairingScreen />;
  }

  return (
    <div className="app-shell flex h-full">
      <Sidebar />

      <main className="relative flex min-w-0 flex-1 flex-col">
        {!online && (
          <div
            className="flex shrink-0 items-center justify-center gap-2 px-3 py-1.5 text-[12px]"
            style={{ background: "var(--warn-soft)", color: "var(--warn)" }}
          >
            <IconOffline className="size-3.5" />
            Connection lost — reconnecting
          </div>
        )}

        <div className="flex min-h-0 flex-1 flex-col">
          {!ready ? (
            <div className="grid h-full place-items-center">
              <Spinner className="size-5 text-[var(--text-faint)]" />
            </div>
          ) : screen === "board" ? (
            <BoardView />
          ) : screen === "run" ? (
            <RunView />
          ) : screen === "models" ? (
            <ModelsView />
          ) : (
            <SettingsView />
          )}
        </div>

        <TabBar />

        {!isDesktop() && !isRemote() && (
          <div className="pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2 max-md:bottom-16">
            <Badge tone="warn">
              Preview — showing sample data, no agents are running
            </Badge>
          </div>
        )}

        <div className="pointer-events-none absolute bottom-4 right-4 flex flex-col items-end gap-2 max-md:bottom-16 max-md:left-4">
          {toasts.map((toast) => (
            <div
              key={toast.id}
              className={cx(
                "enter pointer-events-auto flex max-w-sm items-start gap-2 rounded-lg border px-3 py-2 shadow-lg",
              )}
              style={{
                background: "var(--surface-2)",
                borderColor:
                  toast.level === "error" ? "var(--danger)" : "var(--border-strong)",
              }}
            >
              <p className="text-[12.5px] leading-snug">{toast.message}</p>
              <button
                onClick={() => dismissToast(toast.id)}
                className="mt-0.5 shrink-0 text-[var(--text-faint)] hover:text-[var(--text)]"
                aria-label="Dismiss"
              >
                <IconClose className="size-3.5" />
              </button>
            </div>
          ))}
        </div>
      </main>
    </div>
  );
}

/**
 * The bottom bar on a phone. The sidebar becomes a drawer behind the first
 * tab; the rest go straight to a screen. On a wide window this is not shown —
 * the sidebar is.
 */
function TabBar() {
  const { screen, openScreen, setMenuOpen, menuOpen, runs, selectedProjectId, projects } =
    useStore();
  const waiting = runs.filter((run) => run.status === "waiting").length;
  const active = runs.filter(isActive).length;
  const project = projects.find((p) => p.id === selectedProjectId);

  const tabs: {
    key: string;
    label: string;
    icon: React.ReactNode;
    active: boolean;
    badge?: string;
    badgeTone?: "accent" | "warn";
    onClick: () => void;
  }[] = [
    {
      key: "projects",
      label: project?.name ?? "Projects",
      icon: <IconMenu className="size-5" />,
      active: menuOpen || (screen === "board" && !menuOpen),
      onClick: () => (screen === "board" ? setMenuOpen(!menuOpen) : openScreen("board")),
    },
    {
      key: "runs",
      label: "Runs",
      icon: <IconBoard className="size-5" />,
      active: screen === "run" && !menuOpen,
      badge: waiting > 0 ? String(waiting) : active > 0 ? String(active) : undefined,
      badgeTone: waiting > 0 ? "warn" : "accent",
      onClick: () => openScreen("run"),
    },
    {
      key: "models",
      label: "Models",
      icon: <IconModels className="size-5" />,
      active: screen === "models" && !menuOpen,
      onClick: () => openScreen("models"),
    },
    {
      key: "settings",
      label: "Settings",
      icon: <IconSettings className="size-5" />,
      active: screen === "settings" && !menuOpen,
      onClick: () => openScreen("settings"),
    },
  ];

  return (
    <nav
      className="tab-bar relative z-50 flex shrink-0 border-t md:hidden"
      style={{ background: "var(--surface)" }}
    >
      {tabs.map((tab) => (
        <button
          key={tab.key}
          onClick={tab.onClick}
          className="relative flex min-w-0 flex-1 flex-col items-center gap-0.5 px-1 pb-1 pt-2 text-[10.5px] font-medium"
          style={{ color: tab.active ? "var(--accent-text)" : "var(--text-muted)" }}
        >
          {tab.icon}
          <span className="max-w-full truncate">{tab.label}</span>
          {tab.badge && (
            <span
              className="absolute right-[calc(50%-18px)] top-1 min-w-4 rounded-full px-1 text-center text-[9.5px] font-semibold leading-4 text-white"
              style={{
                background: tab.badgeTone === "warn" ? "var(--warn)" : "var(--accent)",
              }}
            >
              {tab.badge}
            </span>
          )}
        </button>
      ))}
    </nav>
  );
}

/**
 * Shown on a phone that has no token, or one the server refused.
 *
 * The normal way in is the QR code on the desktop's Settings screen, which
 * opens this page with the token already in the link; this is for the person
 * who typed the address by hand.
 */
function PairingScreen() {
  const { pair, pairingError } = useStore();
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);

  return (
    <div className="grid h-full place-items-center px-6">
      <div className="w-full max-w-sm">
        <p className="font-brand text-[28px] font-bold tracking-wide">Heretic</p>
        <h1 className="mt-4 text-[17px] font-semibold tracking-tight">Pair this device</h1>
        <p className="mt-1 text-[13px] leading-snug text-[var(--text-muted)]">
          On the computer running Heretic, open Settings → Remote access and scan
          the code — or paste the token from there.
        </p>

        {pairingError && (
          <p
            className="mt-3 rounded-lg px-3 py-2 text-[12.5px] leading-snug"
            style={{ background: "var(--danger-soft)", color: "var(--danger)" }}
          >
            {pairingError}
          </p>
        )}

        <form
          className="mt-4 flex flex-col gap-2"
          onSubmit={async (event) => {
            event.preventDefault();
            if (!token.trim()) return;
            setBusy(true);
            try {
              await pair(token);
            } finally {
              setBusy(false);
            }
          }}
        >
          <Input
            value={token}
            onChange={(e) => setToken(e.target.value)}
            placeholder="Token"
            autoComplete="off"
            autoCapitalize="off"
            spellCheck={false}
            className="font-mono"
          />
          <Button type="submit" variant="primary" disabled={!token.trim() || busy}>
            {busy ? <Spinner className="size-3.5" /> : "Pair"}
          </Button>
        </form>
      </div>
    </div>
  );
}
