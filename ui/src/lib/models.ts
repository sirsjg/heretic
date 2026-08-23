// Model catalogues for the hosted CLI runners. These are passed verbatim to
// `claude --model` / `codex -m`, so values must be ids the CLIs accept.
// Last refreshed 2026-08 from the Anthropic and OpenAI model docs.

export interface KnownModel {
  value: string;
  label: string;
}

export const CLAUDE_MODELS: KnownModel[] = [
  { value: "claude-opus-5", label: "Claude Opus 5" },
  { value: "claude-sonnet-5", label: "Claude Sonnet 5" },
  { value: "claude-haiku-4-5", label: "Claude Haiku 4.5" },
  { value: "claude-fable-5", label: "Claude Fable 5" },
  { value: "claude-opus-4-8", label: "Claude Opus 4.8" },
  { value: "claude-sonnet-4-6", label: "Claude Sonnet 4.6" },
];

export const CODEX_MODELS: KnownModel[] = [
  { value: "gpt-5.6-sol", label: "GPT-5.6 Sol" },
  { value: "gpt-5.6-terra", label: "GPT-5.6 Terra" },
  { value: "gpt-5.6-luna", label: "GPT-5.6 Luna" },
  { value: "gpt-5.3-codex-spark", label: "GPT-5.3 Codex Spark" },
  { value: "gpt-5.5", label: "GPT-5.5 (legacy)" },
];

/** The catalogue for a runner kind, or null when models are free-form. */
export function knownModelsFor(kind: string): KnownModel[] | null {
  switch (kind) {
    case "claude_code":
      return CLAUDE_MODELS;
    case "codex":
      return CODEX_MODELS;
    default:
      return null;
  }
}

/** Friendly label for a model id, falling back to the raw id. */
export function modelLabel(kind: string, model: string): string {
  const match = knownModelsFor(kind)?.find((m) => m.value === model);
  return match ? match.label : model;
}
