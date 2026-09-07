import { useEffect, useState } from "react";
import { useStore, currentBinding } from "../lib/store";
import type { Settings } from "../lib/types";
import { api } from "../lib/bridge";
import {
  Badge,
  Button,
  Dot,
  Field,
  Input,
  Panel,
  Select,
  Toggle,
} from "../components/ui";
import { IconFolder } from "../components/icons";
import { AccessPanel } from "./AccessPanel";

export function SettingsView() {
  const {
    settings,
    saveSettings,
    saveBinding,
    connection,
    board,
    notify,
    reconnect,
  } = useStore();
  const binding = useStore(currentBinding);
  const [draft, setDraft] = useState<Settings | null>(settings);

  useEffect(() => setDraft(settings), [settings]);

  if (!draft) return null;

  const dirty = JSON.stringify(draft.flux) !== JSON.stringify(settings?.flux);

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex h-14 shrink-0 items-center border-b px-5"
      >
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">Settings</h1>
          <p className="text-[11.5px] text-[var(--text-muted)]">
            Where the board lives, and how this project is worked.
          </p>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          <Panel
            title="Flux server"
            description="Heretic reads projects, epics and tasks from here, and writes progress back."
            actions={
              <span className="flex items-center gap-1.5 text-[11.5px] text-[var(--text-muted)]">
                <Dot tone={connection.connected ? "success" : "danger"} />
                {connection.connected ? "Connected" : "Not connected"}
              </span>
            }
          >
            <div className="grid gap-3 px-4 py-4 sm:grid-cols-2">
              <Field label="Server URL" hint="The Flux web/API address, e.g. http://localhost:3000">
                <Input
                  value={draft.flux.base_url}
                  placeholder="http://localhost:3000"
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      flux: { ...draft.flux, base_url: e.target.value },
                    })
                  }
                />
              </Field>

              <Field
                label="API key"
                hint="Flux servers are locked by default. Leave empty only if yours runs with FLUX_ALLOW_ANONYMOUS."
              >
                <Input
                  type="password"
                  value={draft.flux.api_key ?? ""}
                  placeholder="flx_…"
                  onChange={(e) =>
                    setDraft({
                      ...draft,
                      flux: { ...draft.flux, api_key: e.target.value || null },
                    })
                  }
                />
              </Field>

              <div className="flex items-end gap-2 sm:col-span-2">
                <Button
                  variant="primary"
                  disabled={!dirty}
                  onClick={async () => {
                    await saveSettings(draft);
                    await reconnect();
                  }}
                >
                  Save and reconnect
                </Button>
                <Button onClick={() => void reconnect()}>Test connection</Button>
                {connection.error && (
                  <span className="text-[11.5px]" style={{ color: "var(--danger)" }}>
                    {connection.error}
                  </span>
                )}
              </div>
            </div>
          </Panel>

          <AccessPanel
            flux={draft.flux}
            connection={connection}
            onChange={(flux) => {
              const next = { ...draft, flux };
              setDraft(next);
              void saveSettings(next);
            }}
            onRecheck={() => void reconnect()}
            onNotify={notify}
          />

          {binding && board ? (
            <Panel
              title={`How ${board.project.name} is worked`}
              description="These settings apply to every run in this project."
            >
              <div className="grid gap-3 px-4 py-4 sm:grid-cols-2">
                <Field label="Repository folder">
                  <div className="flex gap-2">
                    <Input value={binding.repo_path} readOnly />
                    <Button
                      icon={<IconFolder className="size-3.5" />}
                      onClick={async () => {
                        const path = await api.chooseFolder();
                        if (path) void saveBinding({ ...binding, repo_path: path });
                      }}
                    >
                      Change
                    </Button>
                  </div>
                </Field>

                <Field
                  label="Base branch"
                  hint="New work forks from here. Empty means whatever the repository has checked out."
                >
                  <Input
                    value={binding.base_branch ?? ""}
                    placeholder="main"
                    onChange={(e) =>
                      void saveBinding({
                        ...binding,
                        base_branch: e.target.value || null,
                      })
                    }
                  />
                </Field>

                <Field
                  label="Where agents work"
                  hint="A worktree per task lets several agents work at once without colliding."
                >
                  <Select
                    value={binding.isolation}
                    onChange={(value) =>
                      void saveBinding({
                        ...binding,
                        isolation: value as typeof binding.isolation,
                      })
                    }
                    options={[
                      { value: "worktree", label: "A git worktree per task" },
                      { value: "in_place", label: "In the folder itself (one at a time)" },
                    ]}
                  />
                </Field>

                <Field
                  label="When a run is approved"
                  hint="Merging requires a clean main checkout; Heretic refuses rather than risk your work."
                >
                  <Select
                    value={binding.integration}
                    disabled={binding.isolation === "in_place"}
                    onChange={(value) =>
                      void saveBinding({
                        ...binding,
                        integration: value as typeof binding.integration,
                      })
                    }
                    options={[
                      { value: "leave", label: "Leave the branch for me to review" },
                      { value: "merge", label: "Merge it into the base branch" },
                    ]}
                  />
                </Field>

                <Field
                  label="Runs at once"
                  hint={
                    binding.isolation === "in_place"
                      ? "Forced to one — agents sharing a folder would overwrite each other."
                      : "How many tasks may be worked in parallel in this project."
                  }
                >
                  <Input
                    type="number"
                    min={1}
                    max={8}
                    disabled={binding.isolation === "in_place"}
                    value={binding.isolation === "in_place" ? 1 : binding.max_parallel}
                    onChange={(e) =>
                      void saveBinding({
                        ...binding,
                        max_parallel: Math.max(1, Number(e.target.value) || 1),
                      })
                    }
                  />
                </Field>

                <Field
                  label="Send back at most"
                  hint="After this many rejected reviews the run stops and asks for a human."
                >
                  <Input
                    type="number"
                    min={0}
                    max={5}
                    value={binding.pipeline.max_revisions}
                    onChange={(e) =>
                      void saveBinding({
                        ...binding,
                        pipeline: {
                          ...binding.pipeline,
                          max_revisions: Math.max(0, Number(e.target.value) || 0),
                        },
                      })
                    }
                  />
                </Field>

                <div className="sm:col-span-2">
                  <p className="mb-2 text-[12px] font-medium">Questions</p>
                  <StageToggle
                    label="Yolo mode"
                    description={
                      binding.pipeline.yolo
                        ? "Agents never stop to ask you anything — required for fully unattended runs."
                        : "An agent that is genuinely blocked may pause its run with a question; the run waits until you answer. An unattended run can sit waiting overnight."
                    }
                    checked={binding.pipeline.yolo}
                    onChange={(yolo) =>
                      void saveBinding({
                        ...binding,
                        pipeline: { ...binding.pipeline, yolo },
                      })
                    }
                  />
                </div>

                <div className="sm:col-span-2">
                  <p className="mb-2 text-[12px] font-medium">Stages</p>
                  <div className="flex flex-col gap-2">
                    <StageToggle
                      label="Plan first"
                      description="The orchestrator writes a brief before any code is touched. Worth it when the implementer is a smaller local model."
                      checked={binding.pipeline.plan}
                      onChange={(plan) =>
                        void saveBinding({
                          ...binding,
                          pipeline: { ...binding.pipeline, plan },
                        })
                      }
                    />
                    <StageToggle
                      label="Review the diff"
                      description="Nothing is marked done until the reviewer approves it."
                      checked={binding.pipeline.review}
                      onChange={(review) =>
                        void saveBinding({
                          ...binding,
                          pipeline: { ...binding.pipeline, review },
                        })
                      }
                    />
                    <StageToggle
                      label="Update the docs"
                      description="A documentation pass after the work is approved."
                      checked={binding.pipeline.document}
                      onChange={(document) =>
                        void saveBinding({
                          ...binding,
                          pipeline: { ...binding.pipeline, document },
                        })
                      }
                    />
                  </div>
                </div>
              </div>
            </Panel>
          ) : (
            <Panel title="Project settings">
              <p className="px-4 py-4 text-[12.5px] text-[var(--text-muted)]">
                Choose a project and set its folder to configure how it is worked.
              </p>
            </Panel>
          )}
        </div>
      </div>
    </div>
  );
}

function StageToggle({
  label,
  description,
  checked,
  onChange,
}: {
  label: string;
  description: string;
  checked: boolean;
  onChange: (next: boolean) => void;
}) {
  return (
    <div
      className="flex items-start gap-3 rounded-lg border px-3 py-2.5"
      style={{ background: "var(--surface-2)" }}
    >
      <Toggle checked={checked} onChange={onChange} label={label} />
      <div className="min-w-0">
        <p className="text-[12.5px] font-medium">
          {label}{" "}
          {!checked && <Badge tone="neutral">off</Badge>}
        </p>
        <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
          {description}
        </p>
      </div>
    </div>
  );
}
