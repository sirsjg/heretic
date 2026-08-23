/**
 * What a finished run consumed: models, tokens, speed and spend.
 *
 * Shown once a run stops. Everything here is derived from the per-stage stats
 * the engine collected while the agents ran — nothing is fetched.
 */

import type { RunRecord, StageStats, TokenUsage } from "../lib/types";
import { STAGE_LABELS } from "../lib/types";

const CHART_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
];

/** Fixed slot per token type, so the colours mean the same thing in every run. */
const TOKEN_TYPES: { key: keyof TokenUsage; label: string; color: string }[] = [
  { key: "input_tokens", label: "Input", color: CHART_COLORS[0]! },
  { key: "output_tokens", label: "Output", color: CHART_COLORS[1]! },
  { key: "cache_read_tokens", label: "Cache read", color: CHART_COLORS[2]! },
  { key: "cache_creation_tokens", label: "Cache write", color: CHART_COLORS[3]! },
];

export function RunStats({ run }: { run: RunRecord }) {
  const stats = run.stats ?? [];
  if (stats.length === 0) return null;

  const totals = emptyUsage();
  let agentMs = 0;
  let cost: number | null = null;
  const byModel = new Map<string, { tokens: number; cost: number | null }>();

  for (const stage of stats) {
    addUsage(totals, stage.usage);
    agentMs += stage.duration_ms;
    if (typeof stage.cost_usd === "number") cost = (cost ?? 0) + stage.cost_usd;
    for (const m of stage.models) {
      const entry = byModel.get(m.model) ?? { tokens: 0, cost: null };
      entry.tokens += totalOf(m.usage);
      if (typeof m.cost_usd === "number") entry.cost = (entry.cost ?? 0) + m.cost_usd;
      byModel.set(m.model, entry);
    }
  }

  const grandTotal = totalOf(totals);
  const seconds = agentMs / 1000;
  const tokensPerSecond = seconds > 0 ? totals.output_tokens / seconds : null;

  const typeSlices = TOKEN_TYPES.map((t) => ({
    label: t.label,
    value: totals[t.key],
    color: t.color,
  })).filter((s) => s.value > 0);

  const modelSlices = foldModels(byModel);

  return (
    <div
      className="enter mt-2 rounded-lg border px-3 py-2.5"
      style={{ background: "var(--surface-2)" }}
    >
      <p className="text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
        Session stats
      </p>

      <div className="mt-2 flex flex-wrap items-baseline gap-x-8 gap-y-2">
        <Stat label="Total tokens" value={formatTokens(grandTotal)} />
        {tokensPerSecond !== null && (
          <Stat
            label="Output tok/s"
            value={formatRate(tokensPerSecond)}
            hint="Output tokens per second of agent time, averaged over the whole run"
          />
        )}
        <Stat label="Agent time" value={formatMs(agentMs)} />
        {cost !== null && <Stat label="Spend" value={formatCost(cost)} />}
      </div>

      <div className="mt-3 flex flex-wrap gap-x-10 gap-y-3 border-t pt-3">
        {typeSlices.length > 0 && (
          <Breakdown title="Tokens by type" slices={typeSlices} />
        )}
        {modelSlices.length >= 2 ? (
          <Breakdown title="Tokens by model" slices={modelSlices} />
        ) : (
          modelSlices.length === 1 && (
            <div className="min-w-0">
              <p className="text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
                Model
              </p>
              <p className="mt-1.5 font-mono text-[11.5px] text-[var(--text-muted)]">
                {modelSlices[0]!.label}
              </p>
            </div>
          )
        )}
      </div>

      <StageTable stats={stats} />
    </div>
  );
}

function Stat({
  label,
  value,
  hint,
}: {
  label: string;
  value: string;
  hint?: string;
}) {
  return (
    <div title={hint}>
      <p className="text-[17px] font-semibold leading-tight tracking-tight">{value}</p>
      <p className="text-[10.5px] uppercase tracking-wider text-[var(--text-faint)]">
        {label}
      </p>
    </div>
  );
}

interface Slice {
  label: string;
  value: number;
  color: string;
}

/** A donut with its legend — the legend carries the names and numbers, the
 * ring carries the proportions. */
function Breakdown({ title, slices }: { title: string; slices: Slice[] }) {
  const total = slices.reduce((sum, s) => sum + s.value, 0);
  if (total === 0) return null;

  return (
    <div className="min-w-0">
      <p className="text-[10.5px] font-semibold uppercase tracking-wider text-[var(--text-faint)]">
        {title}
      </p>
      <div className="mt-1.5 flex items-center gap-3">
        <Donut slices={slices} total={total} />
        <ul className="flex flex-col gap-0.5">
          {slices.map((slice) => (
            <li key={slice.label} className="flex items-center gap-1.5 text-[11.5px]">
              <span
                aria-hidden
                className="size-2 shrink-0 rounded-[2px]"
                style={{ background: slice.color }}
              />
              <span className="text-[var(--text-muted)]">{slice.label}</span>
              <span className="font-mono tabular-nums">{formatTokens(slice.value)}</span>
              <span className="font-mono tabular-nums text-[var(--text-faint)]">
                {Math.round((slice.value / total) * 100)}%
              </span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function Donut({ slices, total }: { slices: Slice[]; total: number }) {
  const size = 76;
  const thickness = 13;
  const c = size / 2;
  const r1 = c - 1;
  const r0 = r1 - thickness;

  // A lone slice is a full ring, which a single arc path cannot draw.
  if (slices.length === 1) {
    return (
      <svg width={size} height={size} role="img" aria-label={slices[0]!.label}>
        <circle
          cx={c}
          cy={c}
          r={(r0 + r1) / 2}
          fill="none"
          stroke={slices[0]!.color}
          strokeWidth={thickness}
        />
      </svg>
    );
  }

  let angle = -Math.PI / 2;
  return (
    <svg width={size} height={size} role="img" aria-label="Breakdown">
      {slices.map((slice) => {
        const sweep = (slice.value / total) * Math.PI * 2;
        const path = arcPath(c, c, r0, r1, angle, angle + sweep);
        angle += sweep;
        return (
          <path
            key={slice.label}
            d={path}
            fill={slice.color}
            stroke="var(--surface-2)"
            strokeWidth={2}
          >
            <title>
              {`${slice.label} — ${formatTokens(slice.value)} (${Math.round((slice.value / total) * 100)}%)`}
            </title>
          </path>
        );
      })}
    </svg>
  );
}

/** An annulus sector from `a0` to `a1` radians. */
function arcPath(
  cx: number,
  cy: number,
  r0: number,
  r1: number,
  a0: number,
  a1: number,
): string {
  const large = a1 - a0 > Math.PI ? 1 : 0;
  const point = (r: number, a: number) =>
    `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
  return [
    `M ${point(r1, a0)}`,
    `A ${r1} ${r1} 0 ${large} 1 ${point(r1, a1)}`,
    `L ${point(r0, a1)}`,
    `A ${r0} ${r0} 0 ${large} 0 ${point(r0, a0)}`,
    "Z",
  ].join(" ");
}

/** The accessible twin of the charts: every number, per stage. */
function StageTable({ stats }: { stats: StageStats[] }) {
  const seen = new Map<string, number>();

  return (
    <div className="mt-3 overflow-x-auto border-t pt-2">
      <table className="w-full text-[11.5px]">
        <thead>
          <tr className="text-left text-[10px] uppercase tracking-wider text-[var(--text-faint)]">
            <th className="py-1 pr-3 font-medium">Stage</th>
            <th className="py-1 pr-3 font-medium">Model</th>
            <th className="py-1 pr-3 text-right font-medium">Time</th>
            <th className="py-1 pr-3 text-right font-medium">In</th>
            <th className="py-1 pr-3 text-right font-medium">Out</th>
            <th className="py-1 pr-3 text-right font-medium">Cached</th>
            <th className="py-1 pr-3 text-right font-medium">Tok/s</th>
            <th className="py-1 text-right font-medium">Cost</th>
          </tr>
        </thead>
        <tbody className="font-mono tabular-nums">
          {stats.map((stage, index) => {
            const pass = (seen.get(stage.stage) ?? 0) + 1;
            seen.set(stage.stage, pass);
            const seconds = stage.duration_ms / 1000;
            const rate = seconds > 0 ? stage.usage.output_tokens / seconds : null;

            return (
              <tr key={index} className="text-[var(--text-muted)]">
                <td className="py-0.5 pr-3 font-sans text-[var(--text)]">
                  {STAGE_LABELS[stage.stage]}
                  {pass > 1 && (
                    <span className="text-[var(--text-faint)]"> ·{pass}</span>
                  )}
                </td>
                <td
                  className="max-w-44 truncate py-0.5 pr-3"
                  title={stage.agent ?? undefined}
                >
                  {stage.models.map((m) => m.model).join(", ") || "—"}
                </td>
                <td className="py-0.5 pr-3 text-right">{formatMs(stage.duration_ms)}</td>
                <td className="py-0.5 pr-3 text-right">
                  {formatTokens(stage.usage.input_tokens)}
                </td>
                <td className="py-0.5 pr-3 text-right">
                  {formatTokens(stage.usage.output_tokens)}
                </td>
                <td
                  className="py-0.5 pr-3 text-right"
                  title="Cache reads + cache writes"
                >
                  {formatTokens(
                    stage.usage.cache_read_tokens + stage.usage.cache_creation_tokens,
                  )}
                </td>
                <td className="py-0.5 pr-3 text-right">
                  {rate !== null && stage.usage.output_tokens > 0
                    ? formatRate(rate)
                    : "—"}
                </td>
                <td className="py-0.5 text-right">
                  {typeof stage.cost_usd === "number" ? formatCost(stage.cost_usd) : "—"}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// --- Aggregation helpers -----------------------------------------------------

function emptyUsage(): TokenUsage {
  return {
    input_tokens: 0,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
  };
}

function addUsage(into: TokenUsage, from: TokenUsage) {
  into.input_tokens += from.input_tokens;
  into.output_tokens += from.output_tokens;
  into.cache_read_tokens += from.cache_read_tokens;
  into.cache_creation_tokens += from.cache_creation_tokens;
}

function totalOf(usage: TokenUsage): number {
  return (
    usage.input_tokens +
    usage.output_tokens +
    usage.cache_read_tokens +
    usage.cache_creation_tokens
  );
}

/** Largest models first, everything past the palette folded into "Other". */
function foldModels(
  byModel: Map<string, { tokens: number; cost: number | null }>,
): Slice[] {
  const ranked = [...byModel.entries()]
    .filter(([, entry]) => entry.tokens > 0)
    .sort((a, b) => b[1].tokens - a[1].tokens);

  const named = ranked.slice(0, CHART_COLORS.length - (ranked.length > CHART_COLORS.length ? 1 : 0));
  const rest = ranked.slice(named.length);

  const slices: Slice[] = named.map(([model, entry], index) => ({
    label: model,
    value: entry.tokens,
    color: CHART_COLORS[index]!,
  }));

  if (rest.length > 0) {
    slices.push({
      label: "Other",
      value: rest.reduce((sum, [, entry]) => sum + entry.tokens, 0),
      color: "var(--text-faint)",
    });
  }

  return slices;
}

// --- Formatting --------------------------------------------------------------

function formatTokens(n: number): string {
  if (n >= 1e6) return `${(n / 1e6).toFixed(n >= 1e7 ? 0 : 1)}M`;
  if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e5 ? 0 : 1)}k`;
  return `${n}`;
}

function formatRate(perSecond: number): string {
  return perSecond >= 10 ? perSecond.toFixed(0) : perSecond.toFixed(1);
}

function formatMs(ms: number): string {
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  return `${Math.floor(minutes / 60)}h ${minutes % 60}m`;
}

function formatCost(usd: number): string {
  return `$${usd.toFixed(usd < 0.1 ? 3 : 2)}`;
}
