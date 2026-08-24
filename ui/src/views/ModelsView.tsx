import { useState } from "react";
import { useStore } from "../lib/store";
import type { ModelProfile, Role, RunnerKind, Settings } from "../lib/types";
import { ROLES, ROLE_BLURBS, ROLE_LABELS } from "../lib/types";
import {
  Badge,
  Button,
  Field,
  Input,
  Panel,
  Select,
  Toggle,
  cx,
} from "../components/ui";
import { IconClose, IconModels } from "../components/icons";
import { DetectPanel } from "./DetectPanel";
import { knownModelsFor, modelLabel } from "../lib/models";

const RUNNER_OPTIONS = [
  { value: "claude_code", label: "Claude Code" },
  { value: "codex", label: "Codex" },
  { value: "codex_oss", label: "Codex — local model (Ollama)" },
  { value: "opencode", label: "OpenCode" },
  { value: "custom", label: "Custom command" },
];

function runnerKey(runner: RunnerKind): string {
  return runner.kind;
}

function runnerDescription(profile: ModelProfile): string {
  switch (profile.runner.kind) {
    case "claude_code":
      return `claude${profile.model ? ` · ${modelLabel("claude_code", profile.model)}` : ""}`;
    case "codex":
      return `codex${profile.model ? ` · ${modelLabel("codex", profile.model)}` : ""}`;
    case "codex_oss":
      return profile.runner.base_url
        ? `codex · ${profile.model ?? "default"} · ${profile.runner.base_url}`
        : `codex --oss · ${profile.model ?? "default"} · Ollama on this machine`;
    case "opencode":
      return profile.runner.base_url
        ? `opencode · ${profile.model ?? "default"} · ${profile.runner.base_url}`
        : `opencode · ${profile.model ?? "its own default"}`;
    case "custom":
      return [profile.runner.command, ...profile.runner.args].join(" ");
  }
}

export function ModelsView() {
  const { settings, saveSettings, notify } = useStore();
  const [editing, setEditing] = useState<string | null>(null);

  if (!settings) return null;

  function update(next: Settings) {
    void saveSettings(next);
  }

  function addProfile() {
    if (!settings) return;
    const id = `profile-${Date.now().toString(36)}`;
    update({
      ...settings,
      profiles: [
        ...settings.profiles,
        {
          id,
          name: "New model",
          runner: { kind: "claude_code" },
          model: null,
          extra_args: [],
          env: {},
          timeout_secs: 3600,
          context_window: null,
          autonomous: true,
        },
      ],
    });
    setEditing(id);
  }

  const profileOptions = settings.profiles.map((p) => ({
    value: p.id,
    label: p.name,
  }));

  return (
    <div className="flex h-full flex-col">
      <header
        data-tauri-drag-region="deep"
        className="flex h-14 shrink-0 items-center border-b px-5"
      >
        <div>
          <h1 className="text-[15px] font-semibold tracking-tight">Models & roles</h1>
          <p className="text-[11.5px] text-[var(--text-muted)]">
            Decide which model does which job on every task.
          </p>
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        <div className="mx-auto flex max-w-3xl flex-col gap-4">
          <DetectPanel
            profiles={settings.profiles}
            onAddProfile={(profile) => {
              // Replace rather than duplicate when the same model is re-added.
              const without = settings.profiles.filter((p) => p.id !== profile.id);
              update({ ...settings, profiles: [...without, profile] });
            }}
            onNotify={notify}
          />

          <Panel
            title="Role assignments"
            description="A run passes through these in order. Give the grind to something cheap and local; keep judgement on your strongest model."
          >
            <div className="flex flex-col">
              {ROLES.map((role) => (
                <RoleRow
                  key={role}
                  role={role}
                  value={settings.roles[role] ?? ""}
                  options={profileOptions}
                  onChange={(profileId) =>
                    update({
                      ...settings,
                      roles: { ...settings.roles, [role]: profileId },
                    })
                  }
                />
              ))}
            </div>
          </Panel>

          <Panel
            title="Model profiles"
            description="Each profile is a command Heretic runs, plus the model it should use."
            actions={
              <Button size="sm" onClick={addProfile}>
                Add profile
              </Button>
            }
          >
            <ul>
              {settings.profiles.map((profile) => (
                <li key={profile.id} className="border-t first:border-t-0">
                  <div className="flex items-center gap-3 px-4 py-3">
                    <IconModels className="size-4 shrink-0 text-[var(--text-faint)]" />
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-[13px] font-medium">{profile.name}</p>
                      <p className="truncate font-mono text-[11.5px] text-[var(--text-muted)]">
                        {runnerDescription(profile)}
                      </p>
                    </div>

                    {profile.runner.kind === "codex_oss" &&
                      !profile.runner.base_url && (
                        <Badge
                          tone="warn"
                          title="No host address, so this runs against Ollama on this machine rather than a server. Re-add the model from the scan to bind it to a host."
                        >
                          No host
                        </Badge>
                      )}

                    {!profile.autonomous && (
                      <Badge tone="warn" title="This model stops to ask for permission, so it cannot run unattended">
                        Asks first
                      </Badge>
                    )}

                    {rolesUsing(settings, profile.id).map((role) => (
                      <Badge key={role} tone="accent">
                        {ROLE_LABELS[role]}
                      </Badge>
                    ))}

                    <Button
                      size="sm"
                      variant="ghost"
                      onClick={() =>
                        setEditing(editing === profile.id ? null : profile.id)
                      }
                    >
                      {editing === profile.id ? "Done" : "Edit"}
                    </Button>
                  </div>

                  {editing === profile.id && (
                    <ProfileEditor
                      profile={profile}
                      inUse={rolesUsing(settings, profile.id).length > 0}
                      onChange={(next) =>
                        update({
                          ...settings,
                          profiles: settings.profiles.map((p) =>
                            p.id === profile.id ? next : p,
                          ),
                        })
                      }
                      onDelete={() => {
                        setEditing(null);
                        update({
                          ...settings,
                          profiles: settings.profiles.filter(
                            (p) => p.id !== profile.id,
                          ),
                          roles: Object.fromEntries(
                            Object.entries(settings.roles).filter(
                              ([, id]) => id !== profile.id,
                            ),
                          ),
                        });
                      }}
                    />
                  )}
                </li>
              ))}
            </ul>
          </Panel>
        </div>
      </div>
    </div>
  );
}

function rolesUsing(settings: Settings, profileId: string): Role[] {
  return ROLES.filter((role) => settings.roles[role] === profileId);
}

function RoleRow({
  role,
  value,
  options,
  onChange,
}: {
  role: Role;
  value: string;
  options: { value: string; label: string }[];
  onChange: (profileId: string) => void;
}) {
  return (
    <div className="flex items-center gap-4 border-t px-4 py-3 first:border-t-0">
      <div className="min-w-0 flex-1">
        <p className="text-[13px] font-medium">{ROLE_LABELS[role]}</p>
        <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
          {ROLE_BLURBS[role]}
        </p>
      </div>
      <div className="w-56 shrink-0">
        <Select
          value={value}
          onChange={onChange}
          options={options}
          placeholder="Not assigned"
        />
      </div>
    </div>
  );
}

function ProfileEditor({
  profile,
  inUse,
  onChange,
  onDelete,
}: {
  profile: ModelProfile;
  inUse: boolean;
  onChange: (next: ModelProfile) => void;
  onDelete: () => void;
}) {
  const kind = runnerKey(profile.runner);
  const knownModels = knownModelsFor(kind);
  // An OpenCode profile bound to a host names the model on its own; left
  // unbound, the model has to carry the provider opencode knows it under.
  const hasHost =
    profile.runner.kind === "opencode" && !!profile.runner.base_url;
  const [customModel, setCustomModel] = useState(false);
  const useCustomModel =
    knownModels !== null &&
    (customModel ||
      (!!profile.model && !knownModels.some((m) => m.value === profile.model)));

  function setRunnerKind(next: string) {
    const runner: RunnerKind =
      next === "codex_oss"
        ? { kind: "codex_oss", base_url: "http://localhost:11434/v1" }
        : next === "opencode"
          ? { kind: "opencode", base_url: null }
          : next === "custom"
            ? { kind: "custom", command: "", args: [] }
            : next === "codex"
              ? { kind: "codex" }
              : { kind: "claude_code" };
    // A model id rarely survives a runner switch, so fall back to the default.
    setCustomModel(false);
    onChange({ ...profile, runner, model: null });
  }

  return (
    <div
      className="grid gap-3 border-t px-4 py-4 sm:grid-cols-2"
      style={{ background: "var(--surface-2)" }}
    >
      <Field label="Name">
        <Input
          value={profile.name}
          onChange={(e) => onChange({ ...profile, name: e.target.value })}
        />
      </Field>

      <Field label="Runs through">
        <Select value={kind} onChange={setRunnerKind} options={RUNNER_OPTIONS} />
      </Field>

      <Field
        label="Model"
        hint={
          knownModels
            ? useCustomModel
              ? "Any model id the CLI accepts."
              : "Default lets the CLI pick its usual model."
            : kind === "codex_oss"
              ? "An Ollama tag, e.g. qwen3-coder:30b. Pull it first with `ollama pull`."
              : kind === "opencode"
                ? hasHost
                  ? "The id the host knows the model by, e.g. qwen3-coder:30b."
                  : "provider/model, as OpenCode writes it — e.g. anthropic/claude-opus-5. Run `opencode models` to see yours."
                : "Leave empty to use the CLI's own default."
        }
      >
        {knownModels ? (
          <div className="flex flex-col gap-2">
            <Select
              value={
                useCustomModel ? "__custom__" : (profile.model ?? "")
              }
              onChange={(value) => {
                if (value === "__custom__") {
                  setCustomModel(true);
                } else {
                  setCustomModel(false);
                  onChange({ ...profile, model: value || null });
                }
              }}
              options={[
                { value: "", label: "Default" },
                ...knownModels,
                { value: "__custom__", label: "Custom…" },
              ]}
            />
            {useCustomModel && (
              <Input
                value={profile.model ?? ""}
                placeholder={
                  kind === "codex" ? "gpt-5.6-sol" : "claude-opus-5"
                }
                onChange={(e) =>
                  onChange({ ...profile, model: e.target.value || null })
                }
              />
            )}
          </div>
        ) : (
          <Input
            value={profile.model ?? ""}
            placeholder={
              kind === "codex_oss" || hasHost
                ? "qwen3-coder:30b"
                : kind === "opencode"
                  ? "anthropic/claude-opus-5"
                  : "default"
            }
            onChange={(e) =>
              onChange({ ...profile, model: e.target.value || null })
            }
          />
        )}
      </Field>

      {profile.runner.kind === "opencode" && (
        <Field
          label="Host endpoint"
          hint="Optional. Point OpenCode at a model server of your own; empty uses the providers it is already set up with."
        >
          <Input
            value={profile.runner.base_url ?? ""}
            placeholder="http://spark.local:11434"
            onChange={(e) =>
              onChange({
                ...profile,
                runner: { kind: "opencode", base_url: e.target.value || null },
              })
            }
          />
        </Field>
      )}

      {profile.runner.kind === "codex_oss" && (
        <Field label="Ollama endpoint" hint="Where the local server is listening.">
          <Input
            value={profile.runner.base_url ?? ""}
            placeholder="http://localhost:11434/v1"
            onChange={(e) =>
              onChange({
                ...profile,
                runner: { kind: "codex_oss", base_url: e.target.value || null },
              })
            }
          />
        </Field>
      )}

      {profile.runner.kind === "custom" && (
        <>
          <Field label="Command">
            <Input
              value={profile.runner.command}
              placeholder="aider"
              onChange={(e) =>
                onChange({
                  ...profile,
                  runner: {
                    kind: "custom",
                    command: e.target.value,
                    args:
                      profile.runner.kind === "custom" ? profile.runner.args : [],
                  },
                })
              }
            />
          </Field>
          <Field
            label="Arguments"
            hint="Space separated. Use {{prompt}} where the task brief goes, and {{model}} for the model id. Without {{prompt}} the brief is sent on stdin."
          >
            <Input
              value={profile.runner.args.join(" ")}
              placeholder="--model {{model}} --message {{prompt}}"
              onChange={(e) =>
                onChange({
                  ...profile,
                  runner: {
                    kind: "custom",
                    command:
                      profile.runner.kind === "custom"
                        ? profile.runner.command
                        : "",
                    args: e.target.value.split(" ").filter(Boolean),
                  },
                })
              }
            />
          </Field>
        </>
      )}

      <Field label="Time limit" hint="Minutes before the agent is stopped. Empty means no limit.">
        <Input
          type="number"
          min={1}
          value={profile.timeout_secs ? Math.round(profile.timeout_secs / 60) : ""}
          onChange={(e) =>
            onChange({
              ...profile,
              timeout_secs: e.target.value
                ? Number(e.target.value) * 60
                : null,
            })
          }
        />
      </Field>

      <div className="flex flex-col gap-1.5">
        <span className="text-[12px] font-medium">Works unattended</span>
        <div className="flex items-center gap-2">
          <Toggle
            checked={profile.autonomous}
            onChange={(autonomous) => onChange({ ...profile, autonomous })}
            label="Works unattended"
          />
          <span className="text-[11.5px] text-[var(--text-muted)]">
            {profile.autonomous
              ? "Acts without asking permission"
              : "Will stop and wait — unusable for automatic runs"}
          </span>
        </div>
      </div>

      <div className="sm:col-span-2 flex justify-end">
        <Button
          size="sm"
          variant="danger"
          icon={<IconClose className="size-3.5" />}
          onClick={onDelete}
          className={cx(inUse && "opacity-60")}
          title={inUse ? "This profile is assigned to a role" : "Delete this profile"}
        >
          Delete profile
        </Button>
      </div>
    </div>
  );
}
