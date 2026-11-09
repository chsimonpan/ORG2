import { describe, expect, it } from "vitest";

import type { KeyInfo } from "@src/api/tauri/rpc/schemas/validation";

import {
  embeddingFingerprint,
  embeddingSelectionError,
  readEmbeddingSelection,
  readRerankSelection,
  selectableModels,
} from "./sessionMemoryModelConfig";

const key = (over: Partial<KeyInfo>): KeyInfo =>
  ({
    id: "k",
    has_api_key: true,
    health_status: "valid",
    enabled_models: [],
    ...over,
  }) as KeyInfo;

describe("selectableModels", () => {
  it("lists only enabled models of the requested purpose on healthy keys", () => {
    const keys = [
      key({
        enabled_models: [
          "qwen/qwen3-vl-embedding",
          "qwen/qwen3-vl-rerank",
          "openai/gpt-6-luna",
        ],
      }),
      key({
        id: "bad",
        health_status: "invalid",
        enabled_models: ["x-embedding"],
      }),
    ];
    expect(selectableModels(keys, "embedding")).toEqual([
      "qwen/qwen3-vl-embedding",
    ]);
    expect(selectableModels(keys, "rerank")).toEqual(["qwen/qwen3-vl-rerank"]);
  });

  it("keeps a saved model that is no longer offered", () => {
    expect(selectableModels([], "embedding", "old-embedding")).toEqual([
      "old-embedding",
    ]);
  });
});

describe("selection helpers", () => {
  it("treats anything but the explicit provider as disabled", () => {
    expect(readEmbeddingSelection({ provider: "auto" }).provider).toBe(
      "disabled"
    );
    expect(readRerankSelection(undefined).provider).toBe("disabled");
  });

  it("changes fingerprint when model or dimensions change", () => {
    const a = readEmbeddingSelection({
      provider: "embedding_api",
      model: "m",
      dimensions: 1024,
    });
    expect(embeddingFingerprint(a)).not.toBe(
      embeddingFingerprint({ ...a, dimensions: 768 })
    );
  });

  it("requires model and dimensions when enabled", () => {
    expect(embeddingSelectionError({ provider: "embedding_api" })).toBe(
      "model"
    );
    expect(
      embeddingSelectionError({ provider: "embedding_api", model: "m" })
    ).toBe("dimensions");
    expect(embeddingSelectionError({ provider: "disabled" })).toBeNull();
  });
});
