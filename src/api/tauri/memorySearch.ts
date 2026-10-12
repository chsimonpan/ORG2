import { invoke } from "@tauri-apps/api/core";
export const HISTORICAL_MEMORY_SOURCE = "openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024";
import type { DurableAnchor } from "@src/features/Journey/types";
/** `sourceAnchor` is a same-owner durable message boundary; when null, `sourceUnavailable` names why (never guessed). */
export interface MemorySearchHit { sourceId: string; content: string; score: number; sourceKind?: string; forkId?: string; reviewId?: string; sourceAnchor?: DurableAnchor | null; sourceUnavailable?: string | null }
/** Query embedding is performed by the registered native handler, never UI callers. */
export function searchOwnerMemory(ownerSessionId: string, query: string, topN = 10): Promise<MemorySearchHit[]> {
 return invoke<MemorySearchHit[]>("memory_search", { ownerSessionId, query, topN });
}
/** Owner-scoped embedding pipeline state. `terminal` failures exhausted `maxAttempts` and are never retried automatically. */
export interface MemoryIndexFailure { sourceId: string; sourceKind: string; attempt: number; terminal: boolean; error: string }
export interface MemoryIndexStatus { queued: number; running: number; completed: number; failed: number; terminalFailed: number; indexed: number; maxAttempts: number; failures: MemoryIndexFailure[] }
export function loadMemoryIndexStatus(ownerSessionId: string): Promise<MemoryIndexStatus> {
 return invoke<MemoryIndexStatus>("memory_index_status", { ownerSessionId });
}
