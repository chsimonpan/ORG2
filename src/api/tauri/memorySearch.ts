import { invoke } from "@tauri-apps/api/core";
export const HISTORICAL_MEMORY_SOURCE = "openai:https://zenmux.ai/api/v1:qwen/qwen3-vl-embedding:1024";
export interface MemorySearchHit { sourceId: string; content: string; score: number }
/** Query embedding is performed by the registered native handler, never UI callers. */
export function searchOwnerMemory(ownerSessionId: string, query: string, topN = 10): Promise<MemorySearchHit[]> {
 return invoke<MemorySearchHit[]>("memory_search", { ownerSessionId, query, topN });
}
