import type { ContextImportCardData } from "../../types";

export function parseContextImportCardResult(
  args: Record<string, unknown>,
  result: Record<string, unknown>
): ContextImportCardData | null {
  const sourceKind =
    (typeof result.source_kind === "string" ? result.source_kind : null) ??
    (typeof args.source_kind === "string" ? args.source_kind : null);
  const sourceId =
    (typeof result.source_id === "string" ? result.source_id : null) ??
    (typeof args.source_id === "string" ? args.source_id : null);
  if (!sourceKind || !sourceId) return null;

  const snapshotId =
    (typeof result.snapshot_id === "string" ? result.snapshot_id : null) ??
    (typeof result.snapshotId === "string" ? result.snapshotId : null) ??
    undefined;
  const namespace =
    (typeof result.namespace === "string" ? result.namespace : null) ??
    `${sourceKind}:${sourceId}`;
  const title =
    (typeof result.title === "string" ? result.title : null) ??
    (typeof args.title === "string" ? args.title : null) ??
    undefined;
  const rawTokenEstimate = result.token_estimate ?? result.tokenEstimate ?? args.token_estimate;
  const tokenEstimate =
    typeof rawTokenEstimate === "number" && Number.isFinite(rawTokenEstimate)
      ? Math.max(0, Math.floor(rawTokenEstimate))
      : undefined;
  const pinned =
    typeof result.pinned === "boolean"
      ? result.pinned
      : typeof args.pinned === "boolean"
        ? args.pinned
        : undefined;

  return {
    snapshotId,
    sourceKind,
    sourceId,
    namespace,
    title,
    tokenEstimate,
    pinned,
  };
}
