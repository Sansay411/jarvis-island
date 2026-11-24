// Types and pure formatting/status helpers for the local-model probe card
// and model dropdown in Settings -> «Модель».

export type Capability = "yes" | "no" | "unknown";
export type Verdict = "ok" | "warn" | "bad";
export type ServerKind = "ollama" | "lmstudio" | "openai";
export type BadgeStatus = "ok" | "warn" | "bad";

export interface ContextInfo {
  modelMax: number | null;
  loaded: number | null;
  needed: number;
  source?: string | null;
}

export interface ModelProbe {
  vision: Capability;
  tools: Capability;
  context: ContextInfo;
  verdict: Verdict;
  summary: string;
  notes: string[];
  serverKind: ServerKind;
}

export interface ModelOption {
  id: string;
  vision: Capability;
  tools: Capability;
  maxContext: number | null;
  loadedContext: number | null;
  state?: string | null;
  suitable: boolean;
  reason?: string | null;
}

export interface ModelsListResult {
  serverKind: ServerKind;
  models: ModelOption[];
}

export interface ProbeBadge {
  id: "vision" | "tools" | "context";
  label: string;
  status: BadgeStatus;
  glyph: "✓" | "⚠" | "✕";
  sub?: string;
}

export function isLocalOrCustomPreset(presetId: string): boolean {
  return presetId === "ollama" || presetId === "lmstudio" || presetId === "custom";
}

/** Format token counts as compact `k` labels (exact powers of 1024 first). */
export function formatTokensK(tokens: number): string {
  if (tokens < 1000) return String(tokens);
  if (tokens % 1024 === 0) return `${tokens / 1024}k`;
  return `${Math.round(tokens / 1000)}k`;
}

export function capabilityStatus(cap: Capability): BadgeStatus {
  if (cap === "yes") return "ok";
  if (cap === "no") return "bad";
  return "warn";
}

export function badgeGlyph(status: BadgeStatus): "✓" | "⚠" | "✕" {
  if (status === "ok") return "✓";
  if (status === "warn") return "⚠";
  return "✕";
}

export function formatContextBadge(context: ContextInfo): { label: string; status: BadgeStatus } {
  const need = formatTokensK(context.needed);
  if (context.loaded != null) {
    const label = `Контекст ${formatTokensK(context.loaded)} из нужных ~${need}`;
    if (context.loaded < context.needed) return { label, status: "bad" };
    if (context.loaded < context.needed + 2048) return { label, status: "warn" };
    return { label, status: "ok" };
  }
  if (context.modelMax != null) {
    if (context.modelMax < context.needed) {
      return { label: `Контекст ${formatTokensK(context.modelMax)} из нужных ~${need}`, status: "bad" };
    }
    return { label: `Контекст до ${formatTokensK(context.modelMax)} (нужно ~${need})`, status: "warn" };
  }
  return { label: `Контекст неизвестен (нужно ~${need})`, status: "warn" };
}

export function buildProbeBadges(probe: ModelProbe, visionModel?: string): ProbeBadge[] {
  const trimmedVision = visionModel?.trim() ?? "";
  const visionViaFallback = probe.vision === "no" && trimmedVision.length > 0;
  const visionStatus: BadgeStatus = visionViaFallback ? "ok" : capabilityStatus(probe.vision);
  const toolsStatus = capabilityStatus(probe.tools);
  const ctx = formatContextBadge(probe.context);

  return [
    {
      id: "vision",
      label: "Видит экран",
      status: visionStatus,
      glyph: badgeGlyph(visionStatus),
      sub: visionViaFallback ? `через ${trimmedVision}` : undefined,
    },
    {
      id: "tools",
      label: "Вызывает инструменты",
      status: toolsStatus,
      glyph: badgeGlyph(toolsStatus),
    },
    {
      id: "context",
      label: ctx.label,
      status: ctx.status,
      glyph: badgeGlyph(ctx.status),
    },
  ];
}
