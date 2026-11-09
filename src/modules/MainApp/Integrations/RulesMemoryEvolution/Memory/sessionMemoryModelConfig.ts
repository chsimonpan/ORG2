import type { KeyInfo } from "@src/api/tauri/rpc/schemas/validation";

export interface EmbeddingSelection {
  provider: "disabled" | "embedding_api";
  model?: string;
  dimensions?: number;
}

export interface RerankSelection {
  provider: "disabled" | "zenmux_api";
  model?: string;
  baseUrl?: string;
  requestTimeoutSecs: number;
}

export const DEFAULT_EMBEDDING_DIMENSIONS = 1024;
const DEFAULT_RERANK_TIMEOUT_SECS = 20;

export type ModelPurpose = "embedding" | "rerank";

/** Key Vault carries no model-type tag, so the purpose is recognised from the model id. */
export function modelMatchesPurpose(
  model: string,
  purpose: ModelPurpose
): boolean {
  return model.toLowerCase().includes(purpose);
}

/**
 * Models the user can pick: enabled by the user on an enabled, healthy Key
 * Vault account. The currently saved value is always kept so a stale choice
 * stays visible instead of silently disappearing.
 */
export function selectableModels(
  keys: readonly KeyInfo[],
  purpose: ModelPurpose,
  current?: string
): string[] {
  const found = new Set<string>();
  for (const key of keys) {
    if (key.health_status !== "valid" || !key.has_api_key) continue;
    for (const model of key.enabled_models) {
      if (modelMatchesPurpose(model, purpose)) found.add(model);
    }
  }
  const sorted = [...found].sort();
  if (current && !found.has(current)) sorted.unshift(current);
  return sorted;
}

export function readEmbeddingSelection(raw: unknown): EmbeddingSelection {
  const value = (raw && typeof raw === "object" ? raw : {}) as Record<
    string,
    unknown
  >;
  const model =
    typeof value.model === "string" && value.model.trim()
      ? value.model.trim()
      : undefined;
  const dimensions =
    typeof value.dimensions === "number" && value.dimensions > 0
      ? value.dimensions
      : undefined;
  return {
    provider: value.provider === "embedding_api" ? "embedding_api" : "disabled",
    model,
    dimensions,
  };
}

export function readRerankSelection(raw: unknown): RerankSelection {
  const value = (raw && typeof raw === "object" ? raw : {}) as Record<
    string,
    unknown
  >;
  return {
    provider: value.provider === "zenmux_api" ? "zenmux_api" : "disabled",
    model:
      typeof value.model === "string" && value.model.trim()
        ? value.model.trim()
        : undefined,
    baseUrl:
      typeof value.baseUrl === "string" && value.baseUrl.trim()
        ? value.baseUrl.trim()
        : undefined,
    requestTimeoutSecs:
      typeof value.requestTimeoutSecs === "number" &&
      value.requestTimeoutSecs > 0
        ? value.requestTimeoutSecs
        : DEFAULT_RERANK_TIMEOUT_SECS,
  };
}

/** Vectors from different model/dimension pairs must never be compared. */
export function embeddingFingerprint(config: EmbeddingSelection): string {
  return `${config.provider}:${config.model ?? ""}:${config.dimensions ?? ""}`;
}

/** An enabled embedding provider needs an explicit model and dimension. */
export function embeddingSelectionError(
  config: EmbeddingSelection
): "model" | "dimensions" | null {
  if (config.provider === "disabled") return null;
  if (!config.model) return "model";
  if (!config.dimensions) return "dimensions";
  return null;
}
