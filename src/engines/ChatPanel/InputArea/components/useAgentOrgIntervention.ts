import { useCallback, useMemo, useState } from "react";

import {
  type AgentOrgMemberIntervention,
  type AgentOrgRunView,
  returnAgentOrgSessionToWork,
} from "@src/api/tauri/agent";
import {
  isCliSession,
  isImportedHistorySession,
} from "@src/util/session/sessionDispatch";

const EMPTY_RESULT = {
  intervention: null as AgentOrgMemberIntervention | null,
  error: null as string | null,
  returning: false,
  refresh: async () => {},
  returnToWork: async () => false,
} as const;

interface AgentOrgInterventionActionState {
  sessionId: string | null;
  error: string | null;
  returning: boolean;
}

export function interventionForSession(
  view: AgentOrgRunView | null,
  sessionId: string | null
): AgentOrgMemberIntervention | null {
  if (!view || !sessionId) return null;
  const member = view.members.find(
    (candidate) =>
      candidate.sessionRuntime?.sessionId === sessionId ||
      (view.context.rootSessionId === sessionId && candidate.isCoordinator)
  );
  return member?.intervention ?? member?.sessionRuntime?.intervention ?? null;
}

/**
 * Derives intervention state from the canonical run view. The old hook polled
 * a second endpoint every 2.5 seconds even though this data is already part of
 * each run-view member projection.
 */
export function useAgentOrgIntervention(
  sessionId: string | null,
  runView: AgentOrgRunView | null,
  refreshRunView: () => Promise<void>
) {
  const [actionState, setActionState] =
    useState<AgentOrgInterventionActionState>({
      sessionId: null,
      error: null,
      returning: false,
    });
  const eligible =
    !!sessionId &&
    !isCliSession(sessionId) &&
    !isImportedHistorySession(sessionId);
  const intervention = interventionForSession(runView, sessionId);

  const returnToWork = useCallback(async () => {
    if (!eligible || !sessionId) return false;
    const currentSessionId = sessionId;
    setActionState({
      sessionId: currentSessionId,
      error: null,
      returning: true,
    });
    try {
      const changed = await returnAgentOrgSessionToWork(currentSessionId);
      return changed;
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : String(error);
      setActionState((previous) =>
        previous.sessionId === currentSessionId
          ? { ...previous, error: message }
          : previous
      );
      return false;
    } finally {
      setActionState((previous) =>
        previous.sessionId === currentSessionId
          ? { ...previous, returning: false }
          : previous
      );
    }
  }, [eligible, sessionId]);

  return useMemo(() => {
    if (!eligible || !sessionId || !runView) return EMPTY_RESULT;
    const stateMatches = actionState.sessionId === sessionId;
    return {
      intervention,
      error: stateMatches ? actionState.error : null,
      returning: stateMatches ? actionState.returning : false,
      refresh: refreshRunView,
      returnToWork,
    };
  }, [
    actionState,
    eligible,
    intervention,
    refreshRunView,
    returnToWork,
    runView,
    sessionId,
  ]);
}
