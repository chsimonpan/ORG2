import React, { memo, useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  type CompactionPrefsView,
  getCompactionPrefs,
  setCompactionThreshold,
  setIdleCompaction,
} from "@src/api/tauri/agent";
import Button from "@src/components/Button";
import NumberInput from "@src/components/NumberInput";
import Switch from "@src/components/Switch";
import { startVisibilityAwarePoller } from "@src/util/time/scheduling/visibilityAwarePoller";

import { formatTokenCount } from "./useContextUsageInfo";

const TOKENS_PER_MILLION = 1_000_000;
const PRESETS_MILLIONS = [1, 5, 10];
const POLL_MS = 10_000;

interface CompactionPrefsCardProps {
  sessionId: string;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function formatClock(ms: number): string {
  return new Date(ms).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

const CompactionPrefsCard: React.FC<CompactionPrefsCardProps> = memo(
  ({ sessionId }) => {
    const { t } = useTranslation();
    const [view, setView] = useState<CompactionPrefsView | null>(null);
    const [draftMillions, setDraftMillions] = useState<number | undefined>();
    const [pending, setPending] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const adopt = useCallback((next: CompactionPrefsView) => {
      setView(next);
      setDraftMillions(next.effectiveThreshold / TOKENS_PER_MILLION);
      setError(null);
    }, []);

    const refresh = useCallback(async () => {
      try {
        const next = await getCompactionPrefs(sessionId);
        // Keep an in-progress threshold edit; only live numbers are replaced.
        setView(next);
        setDraftMillions((current) =>
          current === undefined
            ? next.effectiveThreshold / TOKENS_PER_MILLION
            : current
        );
        setError(null);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    }, [sessionId]);

    useEffect(() => {
      setView(null);
      setDraftMillions(undefined);
      void refresh();
      return startVisibilityAwarePoller(document, refresh, POLL_MS);
    }, [refresh]);

    const mutate = useCallback(
      async (run: () => Promise<CompactionPrefsView>) => {
        if (pending) return;
        setPending(true);
        try {
          adopt(await run());
        } catch (cause) {
          setError(errorMessage(cause));
        } finally {
          setPending(false);
        }
      },
      [adopt, pending]
    );

    if (!view) {
      return error ? (
        <p
          data-testid="context-info-compaction-prefs-error"
          className="mt-2 text-[11px] text-danger-6"
        >
          {t("contextInfo.compactionPrefsError", { message: error })}
        </p>
      ) : null;
    }

    const { idle } = view;
    const spendOff = view.effectiveThreshold === 0;
    const draftTokens =
      draftMillions === undefined
        ? undefined
        : Math.round(draftMillions * TOKENS_PER_MILLION);
    const dirty =
      draftTokens !== undefined && draftTokens !== view.effectiveThreshold;
    const minMillions = view.thresholdMin / TOKENS_PER_MILLION;
    const maxMillions = view.thresholdMax / TOKENS_PER_MILLION;
    const progress = spendOff
      ? 0
      : Math.min(
          100,
          (view.weightedUsedTokens / view.effectiveThreshold) * 100
        );

    const stateLabel = t(`contextInfo.compactionIdleState.${idle.state}`, {
      time: idle.nextDueMs ? formatClock(idle.nextDueMs) : "",
      defaultValue: idle.state,
    });

    return (
      <div
        data-testid="context-info-compaction-prefs"
        className="mt-2 space-y-2"
      >
        <section className="rounded-lg border border-border-2 bg-bg-2 px-3 py-2.5">
          <div className="flex items-center justify-between gap-2">
            <span className="text-[13px] font-semibold text-text-1">
              {t("contextInfo.compactionThresholdTitle")}
            </span>
            <span
              data-testid="context-info-compaction-threshold-source"
              className="text-[11px] text-text-3"
            >
              {t(
                view.thresholdSource === "session"
                  ? "contextInfo.compactionSourceSession"
                  : "contextInfo.compactionSourceDefault"
              )}
            </span>
          </div>
          <p className="mt-1 text-[11px] text-text-3">
            {t("contextInfo.compactionThresholdHint")}
          </p>
          <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-fill-2">
            <div
              data-testid="context-info-compaction-spend-bar"
              className="h-full rounded-full bg-primary-6"
              style={{ width: `${progress}%` }}
            />
          </div>
          <p
            data-testid="context-info-compaction-spend-text"
            className="mt-1 text-[11px] text-text-2"
          >
            {spendOff
              ? t("contextInfo.compactionSpendOff", {
                  used: formatTokenCount(view.weightedUsedTokens),
                })
              : t("contextInfo.compactionSpendUsage", {
                  used: formatTokenCount(view.weightedUsedTokens),
                  limit: formatTokenCount(view.effectiveThreshold),
                })}
          </p>
          <div className="mt-2 flex items-center gap-2">
            <NumberInput
              size="small"
              dataTestId="context-info-compaction-threshold-input"
              value={draftMillions}
              min={minMillions}
              max={maxMillions}
              step={0.5}
              suffix={t("contextInfo.compactionMillionTokens")}
              disabled={pending}
              onValueChange={setDraftMillions}
            />
            <Button
              size="small"
              variant="primary"
              data-testid="context-info-compaction-threshold-apply"
              disabled={pending || !dirty || !draftTokens}
              onClick={() =>
                draftTokens &&
                void mutate(() =>
                  setCompactionThreshold(sessionId, draftTokens)
                )
              }
            >
              {t("contextInfo.compactionApply")}
            </Button>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            {PRESETS_MILLIONS.map((millions) => (
              <Button
                key={millions}
                size="small"
                variant="tertiary"
                data-testid={`context-info-compaction-preset-${millions}`}
                disabled={pending}
                onClick={() =>
                  void mutate(() =>
                    setCompactionThreshold(
                      sessionId,
                      millions * TOKENS_PER_MILLION
                    )
                  )
                }
              >
                {t("contextInfo.compactionPreset", { millions })}
              </Button>
            ))}
            <Button
              size="small"
              variant="tertiary"
              data-testid="context-info-compaction-spend-off"
              disabled={pending || view.weightedThreshold === 0}
              onClick={() =>
                void mutate(() => setCompactionThreshold(sessionId, 0))
              }
            >
              {t("contextInfo.compactionTurnOff")}
            </Button>
            <Button
              size="small"
              variant="tertiary"
              data-testid="context-info-compaction-threshold-reset"
              disabled={pending || view.weightedThreshold === null}
              onClick={() =>
                void mutate(() => setCompactionThreshold(sessionId, null))
              }
            >
              {t("contextInfo.compactionUseDefault", {
                value: formatTokenCount(view.agentDefaultThreshold),
              })}
            </Button>
          </div>
        </section>

        <section className="rounded-lg border border-border-2 bg-bg-2 px-3 py-2.5">
          <div className="flex items-center justify-between gap-2">
            <span className="min-w-0 flex-1">
              <span className="block text-[13px] font-semibold text-text-1">
                {t("contextInfo.compactionIdleTitle")}
              </span>
              <span className="mt-0.5 block text-[11px] text-text-3">
                {t("contextInfo.compactionIdleDescription")}
              </span>
            </span>
            <Switch
              checked={idle.enabled}
              disabled={pending || (!idle.supported && !idle.enabled)}
              size="default"
              ariaLabel={t("contextInfo.compactionIdleToggleAria")}
              dataTestId="context-info-compaction-idle-switch"
              onCheckedChange={(enabled) =>
                void mutate(() =>
                  setIdleCompaction(sessionId, enabled, idle.minutes)
                )
              }
            />
          </div>
          {!idle.supported && (
            <p
              data-testid="context-info-compaction-idle-unsupported"
              className="mt-1 text-[11px] text-warning-6"
            >
              {idle.unsupportedReason ??
                t("contextInfo.compactionIdleUnsupported")}
            </p>
          )}
          <div className="mt-2 flex items-center gap-2">
            <NumberInput
              size="small"
              dataTestId="context-info-compaction-idle-minutes"
              value={idle.minutes}
              min={idle.minutesMin}
              max={idle.minutesMax}
              step={1}
              suffix={t("contextInfo.compactionMinutes")}
              disabled={pending || !idle.supported}
              onValueChange={(minutes) =>
                minutes !== undefined &&
                minutes !== idle.minutes &&
                void mutate(() =>
                  setIdleCompaction(sessionId, idle.enabled, minutes)
                )
              }
            />
          </div>
          {idle.enabled && (
            <p
              data-testid="context-info-compaction-idle-state"
              className="mt-1 text-[11px] text-text-2"
            >
              {stateLabel}
            </p>
          )}
          {idle.lastAttempt && (
            <p
              data-testid="context-info-compaction-idle-last-attempt"
              className="mt-0.5 text-[11px] text-text-3"
            >
              {t("contextInfo.compactionIdleLastAttempt", {
                status: t(
                  `contextInfo.compactionIdleStatus.${idle.lastAttempt.status}`,
                  { defaultValue: idle.lastAttempt.status }
                ),
                time: formatClock(idle.lastAttempt.atMs),
                tokens:
                  idle.lastAttempt.tokensBefore !== null &&
                  idle.lastAttempt.tokensAfter !== null
                    ? t("contextInfo.compactionIdleTokens", {
                        before: formatTokenCount(idle.lastAttempt.tokensBefore),
                        after: formatTokenCount(idle.lastAttempt.tokensAfter),
                      })
                    : "",
              })}
            </p>
          )}
          <p className="mt-1.5 text-[11px] leading-snug text-text-3">
            {t("contextInfo.compactionIdleCost")}
          </p>
        </section>

        {error && (
          <p
            data-testid="context-info-compaction-prefs-error"
            className="text-[11px] text-danger-6"
          >
            {t("contextInfo.compactionPrefsError", { message: error })}
          </p>
        )}
      </div>
    );
  }
);

CompactionPrefsCard.displayName = "CompactionPrefsCard";

export default CompactionPrefsCard;
