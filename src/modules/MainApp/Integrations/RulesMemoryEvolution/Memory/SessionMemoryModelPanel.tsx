import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { rpc } from "@src/api/tauri/rpc";
import type { KeyInfo } from "@src/api/tauri/rpc/schemas/validation";
import Button from "@src/components/Button";
import Input from "@src/components/Input";
import Select, { type SelectOption } from "@src/components/Select";
import {
  SECTION_CONTROL_STYLE,
  SectionContainer,
  SectionRow,
} from "@src/components/layout/Section";
import { createLogger } from "@src/hooks/logger";

import {
  DEFAULT_EMBEDDING_DIMENSIONS,
  type EmbeddingSelection,
  type RerankSelection,
  embeddingFingerprint,
  embeddingSelectionError,
  readEmbeddingSelection,
  readRerankSelection,
  selectableModels,
} from "./sessionMemoryModelConfig";

const log = createLogger("SessionMemoryModelPanel");
const NONE = "__none__";

/**
 * Picks which already-enabled Key Vault models serve semantic recall:
 * one embedding model and one rerank model. No separate model registry.
 */
const SessionMemoryModelPanel: React.FC = () => {
  const { t } = useTranslation("integrations");
  const [embedding, setEmbedding] = useState<EmbeddingSelection>(() =>
    readEmbeddingSelection(undefined)
  );
  const [saved, setSaved] = useState<EmbeddingSelection | null>(null);
  const [rerank, setRerank] = useState<RerankSelection>(() =>
    readRerankSelection(undefined)
  );
  const [keys, setKeys] = useState<KeyInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [reembed, setReembed] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [integrations, listed] = await Promise.all([
        rpc.integrations.get(),
        rpc.validation.listKeys(),
      ]);
      const record = integrations as Record<string, unknown>;
      const nextEmbedding = readEmbeddingSelection(record.embedding);
      setEmbedding(nextEmbedding);
      setSaved(nextEmbedding);
      setRerank(readRerankSelection(record.rerank));
      setKeys(listed);
      setReembed(false);
    } catch (err: unknown) {
      log.error("[SessionMemoryModelPanel] load failed", err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const toOptions = useCallback(
    (models: string[]): SelectOption[] => [
      { value: NONE, label: t("sessionMemoryModels.none") },
      ...models.map((model) => ({ value: model, label: model })),
    ],
    [t]
  );
  const embeddingOptions = useMemo(
    () => toOptions(selectableModels(keys, "embedding", embedding.model)),
    [keys, embedding.model, toOptions]
  );
  const rerankOptions = useMemo(
    () => toOptions(selectableModels(keys, "rerank", rerank.model)),
    [keys, rerank.model, toOptions]
  );

  const missing = embeddingSelectionError(embedding);
  const changed =
    saved != null &&
    embeddingFingerprint(embedding) !== embeddingFingerprint(saved);

  const handleSave = useCallback(async () => {
    setSaving(true);
    setError(null);
    try {
      await rpc.integrations.updatePatch({ patch: { embedding, rerank } });
      if (changed) setReembed(true);
      setSaved(embedding);
    } catch (err: unknown) {
      log.error("[SessionMemoryModelPanel] save failed", err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  }, [embedding, rerank, changed]);

  return (
    <SectionContainer data-testid="session-memory-models">
      <SectionRow
        label={t("sessionMemoryModels.embedding.label")}
        description={t("sessionMemoryModels.embedding.description")}
      >
        <Select
          value={embedding.provider === "disabled" ? NONE : embedding.model}
          options={embeddingOptions}
          disabled={loading}
          style={SECTION_CONTROL_STYLE}
          dataTestId="session-memory-embedding-model"
          onChange={(value) =>
            setEmbedding((previous) =>
              value === NONE
                ? { provider: "disabled" }
                : {
                    provider: "embedding_api",
                    model: String(value),
                    dimensions:
                      previous.dimensions ?? DEFAULT_EMBEDDING_DIMENSIONS,
                  }
            )
          }
        />
      </SectionRow>
      {embedding.provider === "embedding_api" && (
        <SectionRow
          label={t("sessionMemoryModels.dimensions.label")}
          description={t("sessionMemoryModels.dimensions.description")}
        >
          <Input
            value={String(embedding.dimensions ?? "")}
            style={SECTION_CONTROL_STYLE}
            data-testid="session-memory-embedding-dimensions"
            onChange={(raw) => {
              const number = Number(raw);
              setEmbedding((previous) => ({
                ...previous,
                dimensions:
                  Number.isInteger(number) && number > 0 ? number : undefined,
              }));
            }}
          />
        </SectionRow>
      )}
      <SectionRow
        label={t("sessionMemoryModels.rerank.label")}
        description={t("sessionMemoryModels.rerank.description")}
      >
        <Select
          value={rerank.provider === "disabled" ? NONE : rerank.model}
          options={rerankOptions}
          disabled={loading}
          style={SECTION_CONTROL_STYLE}
          dataTestId="session-memory-rerank-model"
          onChange={(value) =>
            setRerank((previous) =>
              value === NONE
                ? { ...previous, provider: "disabled", model: undefined }
                : { ...previous, provider: "zenmux_api", model: String(value) }
            )
          }
        />
      </SectionRow>
      <SectionRow>
        <div className="flex flex-col gap-2">
          {missing && (
            <span className="text-warning-text text-xs">
              {t(`sessionMemoryModels.missing.${missing}`)}
            </span>
          )}
          {reembed && (
            <span className="text-xs text-text-2">
              {t("sessionMemoryModels.reembedNotice")}
            </span>
          )}
          {error && <span className="text-danger-text text-xs">{error}</span>}
          <div>
            <Button
              onClick={() => void handleSave()}
              disabled={loading || saving || missing != null}
              data-testid="session-memory-models-save"
            >
              {t("sessionMemoryModels.save")}
            </Button>
          </div>
        </div>
      </SectionRow>
    </SectionContainer>
  );
};

export default SessionMemoryModelPanel;
