/**
 * Finding out what is actually available to run.
 *
 * Two things get discovered: agent CLIs installed on this machine, and the
 * models each configured host is holding — Ollama on this laptop, or a box on
 * the network such as a DGX Spark. Anything found can be turned into a profile
 * in one click; anything not found can still be added by hand.
 */

import { useCallback, useEffect, useState } from "react";
import type {
  CliStatus,
  DiscoveredModel,
  Environment,
  HostProbe,
  ModelProfile,
  RunnerKind,
} from "../lib/types";
import { describeModel } from "../lib/types";
import { api } from "../lib/bridge";
import { Badge, Button, Dot, Field, Input, Panel, Spinner, cx } from "../components/ui";
import {
  IconAlert,
  IconCheck,
  IconClose,
  IconModels,
  IconRefresh,
} from "../components/icons";

export function DetectPanel({
  profiles,
  onAddProfile,
  onNotify,
}: {
  profiles: ModelProfile[];
  onAddProfile: (profile: ModelProfile) => void;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const [environment, setEnvironment] = useState<Environment | null>(null);
  const [scanning, setScanning] = useState(false);
  const [adding, setAdding] = useState(false);

  const scan = useCallback(async () => {
    setScanning(true);
    try {
      setEnvironment(await api.detectEnvironment());
    } catch (error) {
      onNotify("error", String(error));
    } finally {
      setScanning(false);
    }
  }, [onNotify]);

  useEffect(() => {
    void scan();
  }, [scan]);

  /** Whether a profile already points at this model on this host. */
  function alreadyAdded(model: string, baseUrl: string): boolean {
    return profiles.some(
      (profile) =>
        profile.model === model &&
        profile.runner.kind === "codex_oss" &&
        (profile.runner.base_url ?? "").startsWith(
          baseUrl.replace(/\/+$/, ""),
        ),
    );
  }

  async function addModel(host: HostProbe, model: DiscoveredModel) {
    setAdding(true);
    try {
      const base = await api.openaiBase(host.host.base_url);
      const runner: RunnerKind = { kind: "codex_oss", base_url: base };
      onAddProfile({
        id: `${host.host.id}-${model.id}`.replace(/[^a-zA-Z0-9-]/g, "-"),
        name: `${model.id} (${host.host.name})`,
        runner,
        model: model.id,
        extra_args: [],
        env: {},
        timeout_secs: 5400,
        autonomous: true,
      });
      onNotify("info", `Added ${model.id}.`);
    } catch (error) {
      onNotify("error", String(error));
    } finally {
      setAdding(false);
    }
  }

  function addCli(cli: CliStatus) {
    const runner: RunnerKind =
      cli.program === "claude" ? { kind: "claude_code" } : { kind: "codex" };
    onAddProfile({
      id: cli.program,
      name: cli.label,
      runner,
      model: null,
      extra_args: [],
      env: {},
      timeout_secs: 3600,
      autonomous: true,
    });
    onNotify("info", `Added ${cli.label}.`);
  }

  return (
    <Panel
      title="What's available"
      description="Agent CLIs on this machine, and the models each host is holding."
      actions={
        <Button
          size="sm"
          variant="ghost"
          icon={scanning ? <Spinner className="size-3.5" /> : <IconRefresh className="size-3.5" />}
          disabled={scanning}
          onClick={() => void scan()}
        >
          {scanning ? "Scanning…" : "Scan again"}
        </Button>
      }
    >
      {!environment && scanning && (
        <p className="px-4 py-4 text-[12.5px] text-[var(--text-muted)]">
          Looking for agents and models…
        </p>
      )}

      {environment && (
        <>
          <div className="border-t px-4 py-3">
            <p className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
              Agent CLIs
            </p>
            <div className="flex flex-col gap-1.5">
              {environment.clis.map((cli) => (
                <CliRow
                  key={cli.program}
                  cli={cli}
                  added={profiles.some(
                    (p) =>
                      (cli.program === "claude" && p.runner.kind === "claude_code") ||
                      (cli.program === "codex" && p.runner.kind === "codex"),
                  )}
                  onAdd={() => addCli(cli)}
                />
              ))}
            </div>
          </div>

          <div className="border-t px-4 py-3">
            <p className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
              Model hosts
            </p>
            <div className="flex flex-col gap-2">
              {environment.hosts.map((probe) => (
                <HostBlock
                  key={probe.host.id}
                  probe={probe}
                  busy={adding}
                  isAdded={(model) => alreadyAdded(model, probe.host.base_url)}
                  onAddModel={(model) => void addModel(probe, model)}
                  onRemove={
                    probe.host.id === "local-ollama"
                      ? undefined
                      : async () => {
                          await api.removeHost(probe.host.id);
                          void scan();
                        }
                  }
                />
              ))}
            </div>

            <AddHost
              onAdded={() => void scan()}
              onNotify={onNotify}
            />
          </div>
        </>
      )}
    </Panel>
  );
}

function CliRow({
  cli,
  added,
  onAdd,
}: {
  cli: CliStatus;
  added: boolean;
  onAdd: () => void;
}) {
  return (
    <div
      className="flex items-center gap-2.5 rounded-lg border px-3 py-2"
      style={{ background: "var(--surface-2)" }}
    >
      <Dot tone={cli.found ? "success" : "neutral"} />
      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-medium">{cli.label}</p>
        <p className="truncate font-mono text-[11px] text-[var(--text-muted)]">
          {cli.found ? (cli.version ?? cli.program) : cli.problem}
        </p>
      </div>

      {cli.found &&
        (added ? (
          <Badge tone="success">
            <IconCheck className="size-3" />
            Added
          </Badge>
        ) : (
          <Button size="sm" onClick={onAdd}>
            Add
          </Button>
        ))}
    </div>
  );
}

function HostBlock({
  probe,
  busy,
  isAdded,
  onAddModel,
  onRemove,
}: {
  probe: HostProbe;
  busy: boolean;
  isAdded: (model: string) => boolean;
  onAddModel: (model: DiscoveredModel) => void;
  onRemove?: () => void;
}) {
  const [open, setOpen] = useState(true);

  return (
    <div className="rounded-lg border" style={{ background: "var(--surface-2)" }}>
      <div className="flex items-center gap-2.5 px-3 py-2">
        <Dot tone={probe.reachable ? "success" : "danger"} />
        <button
          className="min-w-0 flex-1 text-left"
          onClick={() => setOpen((value) => !value)}
        >
          <p className="text-[12.5px] font-medium">
            {probe.host.name}{" "}
            {probe.kind === "openai_compatible" && (
              <Badge tone="info" title="Speaks the OpenAI API">
                OpenAI-compatible
              </Badge>
            )}
          </p>
          <p className="truncate font-mono text-[11px] text-[var(--text-muted)]">
            {probe.host.base_url}
            {probe.reachable && ` · ${probe.models.length} models`}
            {probe.version && ` · v${probe.version}`}
          </p>
        </button>

        {onRemove && (
          <Button
            size="sm"
            variant="ghost"
            icon={<IconClose className="size-3.5" />}
            aria-label={`Remove ${probe.host.name}`}
            onClick={onRemove}
          />
        )}
      </div>

      {(probe.problem || probe.warning) && (
        <div className="flex flex-col gap-1.5 px-3 pb-2">
          {[probe.warning, probe.problem].filter(Boolean).map((message) => (
            <div key={message} className="flex items-start gap-2">
              <IconAlert
                className="mt-0.5 size-3.5 shrink-0"
                style={{ color: "var(--warn)" }}
              />
              <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
                {message}
              </p>
            </div>
          ))}
        </div>
      )}

      {open && probe.models.length > 0 && (
        <ul className="border-t">
          {probe.models.map((model) => {
            const added = isAdded(model.id);
            return (
              <li
                key={model.id}
                className="flex items-center gap-2.5 border-b px-3 py-1.5 last:border-b-0"
              >
                <IconModels className="size-3.5 shrink-0 text-[var(--text-faint)]" />
                <div className="min-w-0 flex-1">
                  <p className="truncate font-mono text-[11.5px]">{model.id}</p>
                  {describeModel(model) && (
                    <p className="truncate text-[10.5px] text-[var(--text-faint)]">
                      {describeModel(model)}
                    </p>
                  )}
                </div>
                {added ? (
                  <Badge tone="success">
                    <IconCheck className="size-3" />
                    Added
                  </Badge>
                ) : (
                  <Button size="sm" disabled={busy} onClick={() => onAddModel(model)}>
                    Add
                  </Button>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {open && probe.reachable && probe.models.length === 0 && !probe.problem && (
        <p className="border-t px-3 py-2 text-[11.5px] text-[var(--text-faint)]">
          Reachable, but holding no models. Pull one with{" "}
          <code className="font-mono">ollama pull</code>.
        </p>
      )}
    </div>
  );
}

function AddHost({
  onAdded,
  onNotify,
}: {
  onAdded: () => void;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [checking, setChecking] = useState(false);

  async function add() {
    if (!url.trim()) return;
    setChecking(true);
    try {
      // Check before saving, so a typo is caught here rather than at run time.
      const probe = await api.probeHost(name || "Host", url);
      if (!probe.reachable) {
        onNotify("error", probe.problem ?? "That host could not be reached.");
        return;
      }
      await api.saveHost({
        id: `host-${Date.now().toString(36)}`,
        name: name.trim() || new URL(url).hostname,
        base_url: url.trim(),
      });
      onNotify("info", `Added ${name || url} with ${probe.models.length} models.`);
      setOpen(false);
      setName("");
      setUrl("");
      onAdded();
    } catch (error) {
      onNotify("error", String(error));
    } finally {
      setChecking(false);
    }
  }

  if (!open) {
    return (
      <div className="mt-2">
        <Button size="sm" onClick={() => setOpen(true)}>
          Add a host
        </Button>
      </div>
    );
  }

  return (
    <div
      className={cx("mt-2 grid gap-3 rounded-lg border px-3 py-3 sm:grid-cols-2")}
      style={{ background: "var(--surface-2)" }}
    >
      <Field label="Name" hint="Whatever you call the machine.">
        <Input
          value={name}
          placeholder="DGX Spark"
          onChange={(e) => setName(e.target.value)}
        />
      </Field>

      <Field
        label="Address"
        hint="Ollama listens on 11434. Works with vLLM, LM Studio and anything else OpenAI-shaped."
      >
        <Input
          value={url}
          placeholder="http://spark.local:11434"
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void add()}
        />
      </Field>

      <div className="flex gap-2 sm:col-span-2">
        <Button variant="primary" disabled={checking || !url.trim()} onClick={() => void add()}>
          {checking ? "Checking…" : "Check and add"}
        </Button>
        <Button variant="ghost" onClick={() => setOpen(false)}>
          Cancel
        </Button>
      </div>

      <p className="text-[11px] leading-snug text-[var(--text-faint)] sm:col-span-2">
        A remote Ollama only answers other machines when it is bound beyond
        loopback — set <code className="font-mono">OLLAMA_HOST=0.0.0.0</code> on
        the host and allow the port through its firewall.
      </p>
    </div>
  );
}
