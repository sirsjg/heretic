/**
 * Getting through an identity proxy.
 *
 * A Flux server published on a domain usually sits behind something like
 * Cloudflare Access, oauth2-proxy, Authelia or Pomerium. That proxy
 * authenticates the request before Flux sees it, so there are two credentials in
 * play: the proxy's, and then Flux's own API key.
 */

import { useState } from "react";
import type { ConnectionState, FluxConfig } from "../lib/types";
import { CF_ACCESS_ID, CF_ACCESS_SECRET } from "../lib/types";
import { api, isDesktop } from "../lib/bridge";
import {
  Badge,
  Button,
  Field,
  Input,
  Panel,
  Select,
  cx,
} from "../components/ui";
import { IconAlert, IconCheck, IconClose } from "../components/icons";

type Preset = "none" | "cloudflare" | "bearer" | "custom";

const PRESETS: { value: Preset; label: string }[] = [
  { value: "none", label: "Nothing in front of Flux" },
  { value: "cloudflare", label: "Cloudflare Access service token" },
  { value: "bearer", label: "Bearer token (oauth2-proxy, Pomerium)" },
  { value: "custom", label: "Custom headers" },
];

/** Work out which preset the stored headers represent. */
function presetFor(headers: Record<string, string>): Preset {
  const names = Object.keys(headers).map((n) => n.toLowerCase());
  if (names.includes(CF_ACCESS_ID.toLowerCase())) return "cloudflare";
  if (names.includes("authorization")) return "bearer";
  return names.length > 0 ? "custom" : "none";
}

function headerValue(headers: Record<string, string>, name: string): string {
  const match = Object.keys(headers).find(
    (key) => key.toLowerCase() === name.toLowerCase(),
  );
  return match ? (headers[match] ?? "") : "";
}

export function AccessPanel({
  flux,
  connection,
  onChange,
  onRecheck,
  onNotify,
}: {
  flux: FluxConfig;
  connection: ConnectionState;
  onChange: (next: FluxConfig) => void;
  onRecheck: () => void;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const headers = flux.headers ?? {};
  const [preset, setPreset] = useState<Preset>(presetFor(headers));
  const [signingIn, setSigningIn] = useState(false);

  function setHeaders(next: Record<string, string>) {
    onChange({ ...flux, headers: next });
  }

  function choosePreset(next: Preset) {
    setPreset(next);
    // Switching preset clears the previous scheme's headers, so two
    // credentials can never be sent at once by accident.
    if (next === "none") setHeaders({});
    else if (next === "cloudflare")
      setHeaders({ [CF_ACCESS_ID]: "", [CF_ACCESS_SECRET]: "" });
    else if (next === "bearer") setHeaders({ Authorization: "Bearer " });
    else setHeaders({});
  }

  async function signIn() {
    setSigningIn(true);
    try {
      const message = await api.signIn();
      onNotify("info", message);
      onRecheck();
    } catch (error) {
      onNotify("error", String(error));
    } finally {
      setSigningIn(false);
    }
  }

  const warnings = connection.warnings ?? [];

  return (
    <Panel
      title="Access"
      description="How Accelerate gets past the proxy in front of your Flux server."
      actions={<StatusChip connection={connection} />}
    >
      <div className="grid gap-3 px-4 py-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
          <Field
            label="Proxy credential"
            hint="Used on every request, including the live event stream."
          >
            <Select
              value={preset}
              onChange={(value) => choosePreset(value as Preset)}
              options={PRESETS}
            />
          </Field>
        </div>

        {preset === "cloudflare" && (
          <>
            <Field label="Client ID" hint="Ends in .access from your Access service token.">
              <Input
                value={headerValue(headers, CF_ACCESS_ID)}
                placeholder="abc123.access"
                onChange={(e) =>
                  setHeaders({
                    ...headers,
                    [CF_ACCESS_ID]: e.target.value,
                    [CF_ACCESS_SECRET]: headerValue(headers, CF_ACCESS_SECRET),
                  })
                }
              />
            </Field>
            <Field label="Client secret">
              <Input
                type="password"
                value={headerValue(headers, CF_ACCESS_SECRET)}
                onChange={(e) =>
                  setHeaders({
                    ...headers,
                    [CF_ACCESS_ID]: headerValue(headers, CF_ACCESS_ID),
                    [CF_ACCESS_SECRET]: e.target.value,
                  })
                }
              />
            </Field>
            <p className="text-[11.5px] leading-snug text-[var(--text-muted)] sm:col-span-2">
              Add a service token in Cloudflare Zero Trust, then include it in the
              Access policy for this hostname. Service tokens do not expire, so
              unattended runs keep working.
            </p>
          </>
        )}

        {preset === "bearer" && (
          <>
            <Field
              label="Authorization header"
              hint="Sent verbatim. Include the scheme, e.g. Bearer eyJhbGci…"
            >
              <Input
                type="password"
                value={headerValue(headers, "Authorization")}
                placeholder="Bearer …"
                onChange={(e) => setHeaders({ Authorization: e.target.value })}
              />
            </Field>
            <div className="sm:col-span-2">
              <Callout tone="warn">
                This header is the same one Flux uses for its own API key, so only
                one can be sent. Run Flux with <code>FLUX_ALLOW_ANONYMOUS=1</code>{" "}
                behind the proxy and leave the API key empty — the proxy becomes
                the security boundary.
              </Callout>
            </div>
          </>
        )}

        {preset === "custom" && (
          <div className="sm:col-span-2">
            <HeaderEditor headers={headers} onChange={setHeaders} />
          </div>
        )}

        <div className="sm:col-span-2 border-t pt-3">
          <div className="flex flex-wrap items-center gap-3">
            <div className="min-w-0 flex-1">
              <p className="text-[12.5px] font-medium">
                Browser session{" "}
                {flux.cookie ? (
                  <Badge tone="success">signed in</Badge>
                ) : (
                  <Badge tone="neutral">not signed in</Badge>
                )}
              </p>
              <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
                Signs in through your identity provider in a real browser window
                and keeps the session cookie. Convenient, but sessions expire —
                for unattended runs prefer a service token above.
              </p>
            </div>

            {flux.cookie ? (
              <Button
                size="sm"
                variant="ghost"
                icon={<IconClose className="size-3.5" />}
                onClick={async () => {
                  await api.signOut();
                  onChange({ ...flux, cookie: null });
                  onRecheck();
                }}
              >
                Sign out
              </Button>
            ) : (
              <Button
                size="sm"
                disabled={!isDesktop() || signingIn}
                onClick={() => void signIn()}
                title={
                  isDesktop()
                    ? "Open your identity provider"
                    : "Only available in the desktop app"
                }
              >
                {signingIn ? "Waiting for sign-in…" : "Sign in"}
              </Button>
            )}
          </div>
        </div>

        {(warnings.length > 0 || connection.error) && (
          <div className="flex flex-col gap-2 sm:col-span-2">
            {connection.error && (
              <Callout tone={connection.kind === "proxy_challenge" ? "warn" : "danger"}>
                {connection.error}
              </Callout>
            )}
            {warnings.map((warning) => (
              <Callout key={warning} tone="warn">
                {warning}
              </Callout>
            ))}
          </div>
        )}
      </div>
    </Panel>
  );
}

function HeaderEditor({
  headers,
  onChange,
}: {
  headers: Record<string, string>;
  onChange: (next: Record<string, string>) => void;
}) {
  const rows = Object.entries(headers);

  function rename(oldName: string, newName: string) {
    const next: Record<string, string> = {};
    for (const [name, value] of Object.entries(headers)) {
      next[name === oldName ? newName : name] = value;
    }
    onChange(next);
  }

  return (
    <div className="flex flex-col gap-2">
      <span className="text-[12px] font-medium">Headers</span>

      {rows.length === 0 && (
        <p className="text-[11.5px] text-[var(--text-faint)]">
          No headers yet.
        </p>
      )}

      {rows.map(([name, value]) => (
        <div key={name} className="flex items-center gap-2">
          <Input
            value={name}
            placeholder="Header name"
            className="max-w-[40%]"
            onChange={(e) => rename(name, e.target.value)}
          />
          <Input
            value={value}
            placeholder="Value"
            onChange={(e) => onChange({ ...headers, [name]: e.target.value })}
          />
          <Button
            size="sm"
            variant="ghost"
            icon={<IconClose className="size-3.5" />}
            aria-label={`Remove ${name}`}
            onClick={() => {
              const next = { ...headers };
              delete next[name];
              onChange(next);
            }}
          />
        </div>
      ))}

      <div>
        <Button
          size="sm"
          onClick={() => onChange({ ...headers, "": "" })}
          disabled={Object.hasOwn(headers, "")}
        >
          Add header
        </Button>
      </div>
    </div>
  );
}

function Callout({
  tone,
  children,
}: {
  tone: "warn" | "danger";
  children: React.ReactNode;
}) {
  const colour = tone === "warn" ? "var(--warn)" : "var(--danger)";
  const background = tone === "warn" ? "var(--warn-soft)" : "var(--danger-soft)";

  return (
    <div
      className="flex items-start gap-2 rounded-lg px-3 py-2"
      style={{ background }}
    >
      <IconAlert className="mt-0.5 size-3.5 shrink-0" style={{ color: colour }} />
      <p className="text-[11.5px] leading-relaxed">{children}</p>
    </div>
  );
}

function StatusChip({ connection }: { connection: ConnectionState }) {
  const label =
    connection.kind === "ok" || connection.connected
      ? "Reaching Flux"
      : connection.kind === "proxy_challenge"
        ? "Blocked by the proxy"
        : connection.kind === "flux_auth"
          ? "Flux rejected the key"
          : "Unreachable";

  const tone = connection.connected
    ? "success"
    : connection.kind === "proxy_challenge"
      ? "warn"
      : "danger";

  return (
    <Badge tone={tone}>
      {connection.connected && <IconCheck className={cx("size-3")} />}
      {label}
    </Badge>
  );
}
