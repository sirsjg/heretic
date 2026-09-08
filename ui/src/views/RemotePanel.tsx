/**
 * Settings for watching Heretic from somewhere else: the remote server, and
 * the notifications that say when a run needs a person.
 */

import { useEffect, useState } from "react";
import type { NotifyConfig, RemoteConfig, RemoteStatus, Settings } from "../lib/types";
import {
  emptyNotifyConfig,
  emptyNtfyConfig,
  emptyRemoteConfig,
} from "../lib/types";
import { api, isRemote } from "../lib/bridge";
import { Badge, Button, Dot, Field, Input, Panel, Select, Spinner, Toggle } from "../components/ui";
import { IconCopy, IconRefresh } from "../components/icons";

const ALL_INTERFACES = "0.0.0.0";
const THIS_MACHINE = "127.0.0.1";

export function RemotePanel({
  draft,
  saved,
  setDraft,
  saveSettings,
  onNotify,
}: {
  draft: Settings;
  saved: Settings | null;
  setDraft: (next: Settings) => void;
  saveSettings: (next: Settings) => Promise<void>;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const remote = draft.remote ?? emptyRemoteConfig();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [revealed, setRevealed] = useState(false);
  const dirty =
    JSON.stringify(draft.remote ?? null) !== JSON.stringify(saved?.remote ?? null);

  const update = (patch: Partial<RemoteConfig>) =>
    setDraft({ ...draft, remote: { ...remote, ...patch } });

  async function refreshStatus() {
    try {
      setStatus(await api.remoteStatus());
    } catch {
      // The panel still works without it; the listener just cannot be shown.
    }
  }

  // The listener follows the saved settings, so re-read after every save —
  // and keep re-reading while it is on, since an address can appear (a VPN
  // coming up) without anything being saved.
  useEffect(() => {
    void refreshStatus();
    if (!saved?.remote?.enabled) return;
    const timer = setInterval(() => void refreshStatus(), 5000);
    return () => clearInterval(timer);
  }, [saved?.remote]);

  // Every address the machine has, plus the two catch-alls; a bind value
  // saved for an interface that has since gone is kept so it can be changed.
  const bindOptions = [
    { value: THIS_MACHINE, label: "This machine only (127.0.0.1)" },
    ...(status?.interfaces ?? []).map((iface) => ({
      value: iface.ip,
      label: `${iface.label} — ${iface.ip}`,
    })),
    { value: ALL_INTERFACES, label: "Every network interface (0.0.0.0)" },
  ];
  if (!bindOptions.some((option) => option.value === remote.bind)) {
    bindOptions.push({ value: remote.bind, label: remote.bind });
  }

  const listening = Boolean(status?.listening) && Boolean(saved?.remote?.enabled);

  return (
    <Panel
      title="Remote access"
      description="Serve this same interface to your phone, or any browser, from the machine running Heretic."
      actions={
        saved?.remote?.enabled ? (
          <span className="flex items-center gap-1.5 text-[11.5px] text-[var(--text-muted)]">
            <Dot tone={listening ? "success" : status?.error ? "danger" : "warn"} pulse={listening} />
            {listening ? `Listening on ${status?.listening}` : status?.error ? "Not listening" : "Starting"}
          </span>
        ) : undefined
      }
    >
      <div className="grid gap-3 px-4 py-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
          <div
            className="flex items-start gap-3 rounded-lg border px-3 py-2.5"
            style={{ background: "var(--surface-2)" }}
          >
            <Toggle
              checked={remote.enabled}
              onChange={(enabled) => update({ enabled })}
              label="Remote access"
            />
            <div className="min-w-0">
              <p className="text-[12.5px] font-medium">
                Serve the interface over the network{" "}
                {!remote.enabled && <Badge tone="neutral">off</Badge>}
              </p>
              <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
                A phone that pairs can watch runs, answer an agent's question, and
                merge or discard work — everything this window can do. Anyone with
                the token can do the same, so keep it to a Tailscale address or
                your own network.
              </p>
            </div>
          </div>
        </div>

        <Field
          label="Listen on"
          hint="A Tailscale address reaches your devices from anywhere and nobody else's."
        >
          <Select
            value={remote.bind}
            disabled={!remote.enabled}
            onChange={(bind) => update({ bind })}
            options={bindOptions}
          />
        </Field>

        <Field label="Port">
          <Input
            type="number"
            min={1}
            max={65535}
            disabled={!remote.enabled}
            value={remote.port}
            onChange={(e) =>
              update({ port: Math.min(65535, Math.max(1, Number(e.target.value) || 7411)) })
            }
          />
        </Field>

        <Field
          label="Public address"
          hint="Optional. Where clients reach it when that is not the address above — behind Tailscale Serve or a reverse proxy. Also used for links in notifications."
        >
          <Input
            value={remote.public_url ?? ""}
            disabled={!remote.enabled}
            placeholder="https://heretic.your-tailnet.ts.net"
            onChange={(e) => update({ public_url: e.target.value || null })}
          />
        </Field>

        <div className="flex items-end gap-2">
          <Button
            variant="primary"
            disabled={!dirty}
            onClick={async () => {
              await saveSettings(draft);
              await refreshStatus();
            }}
          >
            Save
          </Button>
        </div>

        {status?.error && saved?.remote?.enabled && (
          <p
            className="rounded-lg px-3 py-2 text-[12px] leading-snug sm:col-span-2"
            style={{ background: "var(--danger-soft)", color: "var(--danger)" }}
          >
            {status.error}
          </p>
        )}

        {isRemote() && (
          <p className="text-[11.5px] leading-snug text-[var(--text-muted)] sm:col-span-2">
            You are looking at this from a paired device. Changing the address or
            port here will drop this connection; pair again from the desktop's
            Settings screen with the new one.
          </p>
        )}

        {!isRemote() && listening && status && (
          <PairingCard
            status={status}
            token={saved?.remote?.token ?? null}
            revealed={revealed}
            onReveal={() => setRevealed(!revealed)}
            onNotify={onNotify}
            onRotate={async () => {
              try {
                await api.rotateRemoteToken();
                onNotify("info", "New token. Every paired device will need to pair again.");
                setDraft({
                  ...draft,
                  remote: { ...remote, token: (await api.getSettings()).remote?.token ?? null },
                });
                await refreshStatus();
              } catch (error) {
                onNotify("error", String(error));
              }
            }}
          />
        )}

        <p className="text-[11.5px] leading-snug text-[var(--text-muted)] sm:col-span-2">
          For a glance without the interface, <code className="font-mono">GET /api/ticker</code>{" "}
          with the token as a bearer returns one line — <em>2 running · 1 waiting for you</em> —
          and <code className="font-mono">/api/status</code> the same as JSON. That is enough for
          a widget, a status bar, or a pair of smart glasses.
        </p>
      </div>
    </Panel>
  );
}

function PairingCard({
  status,
  token,
  revealed,
  onReveal,
  onRotate,
  onNotify,
}: {
  status: RemoteStatus;
  token: string | null;
  revealed: boolean;
  onReveal: () => void;
  onRotate: () => Promise<void>;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const [rotating, setRotating] = useState(false);

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      onNotify("info", `${what} copied.`);
    } catch {
      onNotify("error", `Could not copy the ${what.toLowerCase()}.`);
    }
  }

  return (
    <div
      className="flex flex-col gap-4 rounded-lg border p-4 sm:col-span-2 sm:flex-row"
      style={{ background: "var(--surface-2)" }}
    >
      {status.pairing_qr && (
        <div
          className="qr shrink-0 self-center rounded-lg bg-white p-2 text-black sm:self-start"
          // The SVG comes from the Rust side's QR encoder, not from anything a
          // user typed.
          dangerouslySetInnerHTML={{ __html: status.pairing_qr }}
        />
      )}

      <div className="min-w-0 flex-1">
        <p className="text-[12.5px] font-medium">Pair a phone</p>
        <p className="mt-0.5 text-[11.5px] leading-snug text-[var(--text-muted)]">
          Scan this with the phone's camera. The link carries the token, so it
          pairs on opening — and can be added to the home screen from there.
        </p>

        {status.addresses.length > 0 && (
          <div className="mt-3 flex flex-col gap-1">
            {status.addresses.map((address) => (
              <div key={address.ip} className="flex items-center gap-2 text-[12px]">
                <span className="w-24 shrink-0 truncate text-[var(--text-muted)]">
                  {address.label}
                </span>
                <code className="min-w-0 flex-1 truncate font-mono">{address.url}</code>
                <button
                  className="rounded p-1 text-[var(--text-faint)] hover:bg-[var(--surface-3)] hover:text-[var(--text)]"
                  title="Copy the pairing link for this address"
                  onClick={() => token && void copy(`${address.url}/#token=${token}`, "Pairing link")}
                >
                  <IconCopy className="size-3.5" />
                </button>
              </div>
            ))}
          </div>
        )}

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Button size="sm" onClick={onReveal}>
            {revealed ? "Hide token" : "Show token"}
          </Button>
          {token && (
            <Button
              size="sm"
              icon={<IconCopy className="size-3.5" />}
              onClick={() => void copy(token, "Token")}
            >
              Copy token
            </Button>
          )}
          <Button
            size="sm"
            variant="ghost"
            icon={rotating ? <Spinner className="size-3.5" /> : <IconRefresh className="size-3.5" />}
            disabled={rotating}
            title="Mint a new token. Every paired device is logged out."
            onClick={async () => {
              setRotating(true);
              try {
                await onRotate();
              } finally {
                setRotating(false);
              }
            }}
          >
            Rotate token
          </Button>
        </div>
        {revealed && token && (
          <code className="mt-2 block break-all rounded px-2 py-1 font-mono text-[11px]" style={{ background: "var(--surface-3)" }}>
            {token}
          </code>
        )}
      </div>
    </div>
  );
}

/**
 * Push notifications. The desktop posts them; a phone receives them through
 * the ntfy or Pushover app, which is what makes a lock-screen buzz possible
 * without a native app of our own.
 */
export function NotificationsPanel({
  draft,
  saved,
  setDraft,
  saveSettings,
  onNotify,
}: {
  draft: Settings;
  saved: Settings | null;
  setDraft: (next: Settings) => void;
  saveSettings: (next: Settings) => Promise<void>;
  onNotify: (level: "info" | "error", message: string) => void;
}) {
  const notifications = draft.notifications ?? emptyNotifyConfig();
  const ntfy = notifications.ntfy ?? emptyNtfyConfig();
  const pushover = notifications.pushover ?? { user_key: "", app_token: "" };
  const [testing, setTesting] = useState(false);
  const dirty =
    JSON.stringify(draft.notifications ?? null) !==
    JSON.stringify(saved?.notifications ?? null);

  const update = (patch: Partial<NotifyConfig>) =>
    setDraft({ ...draft, notifications: { ...notifications, ...patch } });

  const configured =
    Boolean(saved?.notifications?.ntfy?.topic) ||
    Boolean(saved?.notifications?.pushover?.user_key && saved?.notifications?.pushover?.app_token);

  return (
    <Panel
      title="Notifications"
      description="A push to your phone when a run needs you: an agent's question, a failure, or work waiting on a branch."
      actions={
        configured ? (
          <span className="flex items-center gap-1.5 text-[11.5px] text-[var(--text-muted)]">
            <Dot tone="success" />
            On
          </span>
        ) : undefined
      }
    >
      <div className="grid gap-3 px-4 py-4 sm:grid-cols-2">
        <div className="sm:col-span-2">
          <p className="text-[12px] font-medium">ntfy</p>
          <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
            Free, open source, and self-hostable. Install the ntfy app, subscribe
            it to a topic nobody would guess, and put the same topic here.
          </p>
        </div>
        <Field label="Topic" hint="e.g. heretic-4f9a2c — on the public server, the topic is the only secret.">
          <Input
            value={ntfy.topic}
            placeholder="heretic-…"
            autoCapitalize="off"
            onChange={(e) => update({ ntfy: { ...ntfy, topic: e.target.value.trim() } })}
          />
        </Field>
        <Field label="Server">
          <Input
            value={ntfy.server}
            placeholder="https://ntfy.sh"
            onChange={(e) => update({ ntfy: { ...ntfy, server: e.target.value } })}
          />
        </Field>
        <Field label="Access token" hint="Only for a protected topic or a self-hosted server that needs one.">
          <Input
            type="password"
            value={ntfy.token ?? ""}
            placeholder="tk_…"
            onChange={(e) => update({ ntfy: { ...ntfy, token: e.target.value || null } })}
          />
        </Field>

        <div className="sm:col-span-2">
          <p className="mt-2 text-[12px] font-medium">Pushover</p>
          <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
            A one-off purchase with a polished app. Create an application in your
            Pushover dashboard and paste its token with your user key.
          </p>
        </div>
        <Field label="User key">
          <Input
            type="password"
            value={pushover.user_key}
            placeholder="u…"
            onChange={(e) => update({ pushover: { ...pushover, user_key: e.target.value.trim() } })}
          />
        </Field>
        <Field label="Application token">
          <Input
            type="password"
            value={pushover.app_token}
            placeholder="a…"
            onChange={(e) => update({ pushover: { ...pushover, app_token: e.target.value.trim() } })}
          />
        </Field>

        <div className="sm:col-span-2">
          <div
            className="flex items-start gap-3 rounded-lg border px-3 py-2.5"
            style={{ background: "var(--surface-2)" }}
          >
            <Toggle
              checked={notifications.on_success}
              onChange={(on_success) => update({ on_success })}
              label="Successes too"
            />
            <div className="min-w-0">
              <p className="text-[12.5px] font-medium">Say when a run completes cleanly</p>
              <p className="text-[11.5px] leading-snug text-[var(--text-muted)]">
                Off, a merged run is silent and only the moments needing a decision
                get through. Work left on a branch is always announced — it is a
                decision.
              </p>
            </div>
          </div>
        </div>

        <div className="flex items-end gap-2 sm:col-span-2">
          <Button variant="primary" disabled={!dirty} onClick={() => void saveSettings(draft)}>
            Save
          </Button>
          <Button
            disabled={!configured || dirty || testing}
            title={dirty ? "Save first" : "Post a test message"}
            onClick={async () => {
              setTesting(true);
              try {
                await api.testNotifications();
                onNotify("info", "Sent. Check your phone.");
              } catch (error) {
                onNotify("error", String(error));
              } finally {
                setTesting(false);
              }
            }}
          >
            {testing ? <Spinner className="size-3.5" /> : "Send a test"}
          </Button>
        </div>
      </div>
    </Panel>
  );
}
