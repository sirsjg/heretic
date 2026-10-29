/** Announcing a new release, and installing it. */

import { api } from "../lib/bridge";
import { useStore } from "../lib/store";
import { isActive } from "../lib/types";
import { Button } from "./ui";
import { IconClose, IconDownload } from "./icons";

/** The card at the foot of the sidebar while a newer release is waiting. */
export function UpdateCard() {
  const { update, updateStatus, updateDismissed, dismissUpdate } = useStore();
  if (!update || updateDismissed === update.version) return null;

  return (
    <div
      className="enter mx-2 mb-2 rounded-lg border p-2.5"
      style={{ background: "var(--accent-soft)", borderColor: "var(--border-strong)" }}
    >
      <div className="flex items-start gap-2">
        <IconDownload
          className="mt-0.5 size-3.5 shrink-0"
          style={{ color: "var(--accent-text)" }}
        />
        <div className="min-w-0 flex-1">
          <p className="text-[12px] font-medium leading-tight">
            Heretic {update.version} is available
          </p>
          <p className="text-[11px] leading-tight text-[var(--text-faint)]">
            You have {update.current_version}
          </p>
        </div>
        {updateStatus !== "installing" && (
          <button
            onClick={dismissUpdate}
            className="shrink-0 text-[var(--text-faint)] hover:text-[var(--text)]"
            aria-label="Not now"
            title="Not now"
          >
            <IconClose className="size-3" />
          </button>
        )}
      </div>
      <UpdateActions className="mt-2" />
    </div>
  );
}

/**
 * Install (or, where this copy cannot replace itself, download) the update
 * found, and read what changed. Renders nothing when there is no update.
 */
export function UpdateActions({ className }: { className?: string }) {
  const { update, updateStatus, updateProgress, installUpdate, runs } = useStore();
  if (!update) return null;

  const installing = updateStatus === "installing";
  const active = runs.filter(isActive).length;

  let label = "Install and restart";
  if (installing) {
    const { downloaded = 0, total = null } = updateProgress ?? {};
    label = !total
      ? "Downloading…"
      : downloaded < total
        ? `Downloading ${Math.floor((downloaded / total) * 100)}%`
        : "Installing…";
  }

  return (
    <div className={className}>
      <div className="flex flex-wrap gap-1.5">
        {update.can_install ? (
          <Button
            size="sm"
            variant="primary"
            disabled={installing || active > 0}
            onClick={() => void installUpdate()}
          >
            {label}
          </Button>
        ) : (
          <Button
            size="sm"
            variant="primary"
            icon={<IconDownload className="size-3.5" />}
            onClick={() => void api.openUrl(update.release_url)}
          >
            Download
          </Button>
        )}
        <Button
          size="sm"
          variant="ghost"
          disabled={installing}
          onClick={() => void api.openUrl(update.release_url)}
        >
          What's new
        </Button>
      </div>
      {update.can_install && active > 0 && !installing && (
        <p className="mt-1.5 text-[11px] leading-snug text-[var(--text-muted)]">
          Updating restarts Heretic. Finish or stop the{" "}
          {active === 1 ? "run" : `${active} runs`} in progress first.
        </p>
      )}
      {!update.can_install && (
        <p className="mt-1.5 text-[11px] leading-snug text-[var(--text-muted)]">
          This copy is managed by your package manager, so it can't update itself.
        </p>
      )}
    </div>
  );
}
