import { useEffect } from "react";
import { useStore } from "./lib/store";
import { isDesktop } from "./lib/bridge";
import { Sidebar } from "./components/Sidebar";
import { BoardView } from "./views/BoardView";
import { RunView } from "./views/RunView";
import { ModelsView } from "./views/ModelsView";
import { SettingsView } from "./views/SettingsView";
import { Badge, Spinner, cx } from "./components/ui";
import { IconClose } from "./components/icons";

export default function App() {
  const { ready, screen, initialise, toasts, dismissToast } = useStore();

  useEffect(() => {
    void initialise();
  }, [initialise]);

  return (
    <div className="flex h-full">
      <Sidebar />

      <main className="relative flex min-w-0 flex-1 flex-col">
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

        {!isDesktop() && (
          <div className="pointer-events-none absolute bottom-3 left-1/2 -translate-x-1/2">
            <Badge tone="warn">
              Preview — showing sample data, no agents are running
            </Badge>
          </div>
        )}

        <div className="pointer-events-none absolute bottom-4 right-4 flex flex-col items-end gap-2">
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
