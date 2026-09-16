/**
 * The HTTP transport: what the interface uses when it is served by
 * `heretic-server` rather than shown in the desktop window.
 *
 * Commands go to `POST /api/call/<name>` with the same names and arguments the
 * desktop sends over Tauri's IPC; events arrive on a WebSocket. The bearer
 * token comes from the pairing link's fragment the first time, and lives in
 * localStorage after that.
 */

import type { EngineEvent, FluxEvent } from "./types";

const TOKEN_KEY = "heretic.remote.token";

/** Thrown when the server refuses the token, so the interface can re-pair. */
export class Unauthorised extends Error {
  constructor(message: string) {
    super(message);
    this.name = "Unauthorised";
  }
}

export function readToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function storeToken(token: string | null) {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch {
    // A private window may refuse; the token then lasts the page's lifetime.
  }
  memoryToken = token;
}

let memoryToken: string | null = null;

function token(): string | null {
  return memoryToken ?? readToken();
}

/**
 * Lift a token out of the pairing link's fragment, and a run to open, then
 * clear the fragment so neither sits in the address bar or the history.
 */
export function adoptFragment(): { run: string | null } {
  if (typeof location === "undefined") return { run: null };
  const fragment = location.hash.replace(/^#/, "");
  if (!fragment) return { run: null };
  const params = new URLSearchParams(fragment);
  const pairing = params.get("token");
  if (pairing) storeToken(pairing.trim());
  const run = params.get("run");
  if (pairing || run) {
    history.replaceState(null, "", location.pathname + location.search);
  }
  return { run };
}

export function paired(): boolean {
  return Boolean(token());
}

/** Run one command on the server. */
export async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const bearer = token();
  if (!bearer) throw new Unauthorised("Not paired.");

  const response = await fetch(`/api/call/${command}`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      authorization: `Bearer ${bearer}`,
    },
    body: JSON.stringify(args ?? {}),
  });

  if (response.status === 401) {
    throw new Unauthorised(await errorText(response));
  }
  if (!response.ok) {
    throw new Error(await errorText(response));
  }
  return (await response.json()) as T;
}

async function errorText(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: string };
    if (body.error) return body.error;
  } catch {
    // Not JSON; fall through to the status line.
  }
  return `The server answered ${response.status}.`;
}

// --- Events ------------------------------------------------------------------

type Envelope =
  | { channel: "engine"; event: EngineEvent }
  | { channel: "flux"; event: FluxEvent };

export interface Connectivity {
  /** Whether the event socket is open right now. */
  online: boolean;
  /** True on the reconnect after a drop: events were missed, so re-read. */
  recovered: boolean;
}

type Listener<T> = (event: T) => void;

const engineListeners = new Set<Listener<EngineEvent>>();
const fluxListeners = new Set<Listener<FluxEvent>>();
const connectivityListeners = new Set<Listener<Connectivity>>();

let socket: WebSocket | null = null;
let everConnected = false;
let wasOnline = false;
let retryTimer: ReturnType<typeof setTimeout> | null = null;
let retryDelay = 1000;

function ensureSocket() {
  if (socket || retryTimer) return;
  const bearer = token();
  if (!bearer) return;

  const scheme = location.protocol === "https:" ? "wss" : "ws";
  const url = `${scheme}://${location.host}/api/events?token=${encodeURIComponent(bearer)}`;
  const ws = new WebSocket(url);
  socket = ws;

  ws.onopen = () => {
    retryDelay = 1000;
    const recovered = everConnected;
    everConnected = true;
    wasOnline = true;
    for (const listener of connectivityListeners) listener({ online: true, recovered });
  };

  ws.onmessage = (message) => {
    let envelope: Envelope;
    try {
      envelope = JSON.parse(String(message.data)) as Envelope;
    } catch {
      return;
    }
    if (envelope.channel === "engine") {
      for (const listener of engineListeners) listener(envelope.event);
    } else if (envelope.channel === "flux") {
      for (const listener of fluxListeners) listener(envelope.event);
    }
  };

  ws.onclose = () => {
    socket = null;
    if (wasOnline) {
      wasOnline = false;
      for (const listener of connectivityListeners) {
        listener({ online: false, recovered: false });
      }
    }
    // Nobody listening means nobody to reconnect for.
    if (engineListeners.size + fluxListeners.size + connectivityListeners.size === 0) return;
    retryTimer = setTimeout(() => {
      retryTimer = null;
      ensureSocket();
    }, retryDelay);
    retryDelay = Math.min(retryDelay * 2, 15000);
  };

  ws.onerror = () => {
    // onclose follows and schedules the retry.
  };
}

function subscribe<T>(set: Set<Listener<T>>, listener: Listener<T>): () => void {
  set.add(listener);
  ensureSocket();
  return () => {
    set.delete(listener);
  };
}

export function onEngineEvent(listener: Listener<EngineEvent>): () => void {
  return subscribe(engineListeners, listener);
}

export function onFluxEvent(listener: Listener<FluxEvent>): () => void {
  return subscribe(fluxListeners, listener);
}

export function onConnectivity(listener: Listener<Connectivity>): () => void {
  return subscribe(connectivityListeners, listener);
}

/** Drop the socket so it reopens with the current token. */
export function reconnect() {
  if (retryTimer) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
  retryDelay = 1000;
  if (socket) socket.close();
  else ensureSocket();
}
