import type {
  EvidenceClass,
  JourneyCoverage,
  JourneyGraphEdge,
  JourneyGraphNode,
  JourneyGraphPayload,
} from "@src/api/tauri/journeyGraph";

export interface JourneyEvidence {
  evidenceClass: EvidenceClass;
  sourceRef: string;
}

export interface JourneyDisplayNode extends JourneyEvidence {
  id: string;
  title: string;
  kind: string;
  displayTimestamp?: string | null;
  agentIdentity?: string | null;
  agentBand?: string | null;
  topicTags: string[];
  resultSummary?: string | null;
  lifecycleStatus?: string | null;
  branch?: string | null;
}

export interface JourneyViewModel {
  nodes: JourneyDisplayNode[];
  files: string[];
}

export interface StorylineMilestone extends JourneyDisplayNode {
  sequence: number | null;
}

export interface StorylineIdleGap {
  kind: "idleGap";
  fromTimestamp: string;
  toTimestamp: string;
  durationMs: number;
}

export type StorylineLaneState = "active" | "paused" | "completed" | "unknown";

export interface StorylineLane {
  id: string;
  label: string;
  milestones: StorylineMilestone[];
  gaps: StorylineIdleGap[];
  /** Explicit lifecycle classification; absence is never guessed from time. */
  state?: StorylineLaneState;
  /** A factual lineage link places this lane below its parent lane. */
  parentLaneId?: string;
  /** Explicit source-system branch label, when this is a session lane. */
  branch?: string | null;
  /** True only for a terminal active session with no factual child lane. */
  isActiveTrunk?: boolean;
}

export interface StorylineConnector extends JourneyEvidence {
  from: string;
  to: string;
  kind: string;
}

export interface StorylineViewModel {
  lanes: StorylineLane[];
  connectors: StorylineConnector[];
  unpositioned: StorylineMilestone[];
}

export interface BranchLink extends JourneyEvidence {
  from: string;
  to: string;
  kind: "forkedFrom" | "resumedFrom" | "compactedTo";
}

export interface BranchesViewModel {
  nodes: JourneyDisplayNode[];
  links: BranchLink[];
}

export interface FileLineageLink extends JourneyEvidence {
  from: string;
  to: string;
  kind: "produced" | "modified";
}

export interface FileLineageViewModel {
  files: JourneyDisplayNode[];
  adjacentNodes: JourneyDisplayNode[];
  links: FileLineageLink[];
}

export interface CoverageLedgerEntry extends JourneyCoverage {
  statusKind: "represented" | "mergedInto" | "excluded" | "uncovered";
  detail: string;
}

export interface CoverageLedgerViewModel {
  entries: CoverageLedgerEntry[];
  summary: Record<CoverageLedgerEntry["statusKind"], number>;
  /** P1 does not serialize the independent audit result, so this must not be inferred. */
  provenanceAudit: "notProvided";
}

export const IDLE_GAP_MS = 15 * 60 * 1000;

function assertCompleteGraph(graph: JourneyGraphPayload): void {
  if (graph.coverage.some((entry) => entry.status === "uncovered")) {
    throw new Error("Journey graph coverage is incomplete");
  }
  for (const item of [...graph.nodes, ...graph.edges]) {
    if (!item.evidenceClass || !item.sourceRef) {
      throw new Error("Journey graph is missing required evidence");
    }
  }
}

function titleFromId(id: string): string {
  return id.split("/").slice(1).join("/") || id;
}

function toDisplayNode(node: JourneyGraphNode): JourneyDisplayNode {
  return {
    id: node.id,
    title: node.displayTitle?.trim() || titleFromId(node.id),
    kind: node.kind,
    evidenceClass: node.evidenceClass,
    sourceRef: node.sourceRef,
    displayTimestamp: node.displayTimestamp,
    agentIdentity: node.metadata?.agentIdentity ?? null,
    agentBand: node.metadata?.agentBand ?? null,
    topicTags: node.metadata?.topicTags ?? [],
    resultSummary: node.resultSummary,
    lifecycleStatus: node.lifecycleStatus,
    branch: node.branch,
  };
}

function compareNodes(left: JourneyGraphNode, right: JourneyGraphNode): number {
  return left.id.localeCompare(right.id);
}

function compareEdges(left: JourneyGraphEdge, right: JourneyGraphEdge): number {
  return (
    left.from.localeCompare(right.from) ||
    left.to.localeCompare(right.to) ||
    left.kind.localeCompare(right.kind) ||
    left.sourceRef.localeCompare(right.sourceRef)
  );
}

function parseTimestamp(value: string | null | undefined): number | null {
  if (!value) return null;
  const timestamp = Date.parse(value);
  return Number.isNaN(timestamp) ? null : timestamp;
}

function turnSequence(id: string): number | null {
  const match = /^turn\/[^/]+\/(\d+)$/.exec(id);
  return match ? Number(match[1]) : null;
}

function sessionIdForNode(
  node: JourneyGraphNode,
  edges: JourneyGraphEdge[]
): string | null {
  if (node.kind === "session") return node.id;
  const owner = edges.find(
    (edge) =>
      edge.to === node.id &&
      edge.kind === "contains" &&
      edge.from.startsWith("session/")
  );
  return owner?.from ?? null;
}

/** Pure presentation mapping. It cannot infer or repair graph facts. */
export function graphToJourneyViewModel(
  graph: JourneyGraphPayload
): JourneyViewModel {
  assertCompleteGraph(graph);
  const nodes = [...graph.nodes].sort(compareNodes).map(toDisplayNode);
  return {
    nodes,
    files: nodes.filter((node) => node.kind === "file").map((node) => node.id),
  };
}

/**
 * Builds session lanes from explicit contains + factual lineage edges only.
 * A lane can be active or paused only when canonical lifecycle state says so;
 * time distance merely compresses the axis and never creates a branch.
 */
function laneState(status: string | null | undefined): StorylineLaneState {
  if (status === "running" || status === "working" || status === "pending")
    return "active";
  if (
    status === "interrupted" ||
    status === "failed" ||
    status === "cancelled" ||
    status === "abandoned"
  )
    return "paused";
  if (status === "completed") return "completed";
  return "unknown";
}

function factualParentSessions(edges: JourneyGraphEdge[]): Map<string, string> {
  const parents = new Map<string, string>();
  for (const edge of edges) {
    if (!["forkedFrom", "resumedFrom", "compactedTo"].includes(edge.kind))
      continue;
    if (!edge.from.startsWith("session/") || !edge.to.startsWith("session/"))
      continue;
    // The graph projector emits child -> parent for all lineage edge kinds.
    parents.set(edge.from, edge.to);
  }
  return parents;
}

export function graphToStorylineViewModel(
  graph: JourneyGraphPayload,
  idleGapMs = IDLE_GAP_MS
): StorylineViewModel {
  assertCompleteGraph(graph);
  const edges = [...graph.edges].sort(compareEdges);
  const laneMembers = new Map<string, JourneyGraphNode[]>();
  const unpositioned: StorylineMilestone[] = [];
  const sessionNodes = new Map(
    graph.nodes
      .filter((node) => node.kind === "session")
      .map((node) => [node.id, node])
  );
  const parentBySession = factualParentSessions(edges);
  const childSessions = new Set(parentBySession.values());

  for (const node of [...graph.nodes].sort(compareNodes)) {
    const timestamp = parseTimestamp(node.displayTimestamp);
    if (timestamp === null) {
      unpositioned.push({
        ...toDisplayNode(node),
        sequence: turnSequence(node.id),
      });
      continue;
    }
    const laneId = sessionIdForNode(node, edges) ?? "unlinked-facts";
    const lane = laneMembers.get(laneId) ?? [];
    lane.push(node);
    laneMembers.set(laneId, lane);
  }

  const laneDepth = (id: string): number => {
    let depth = 0;
    let cursor = parentBySession.get(id);
    const visited = new Set<string>([id]);
    while (cursor && !visited.has(cursor)) {
      visited.add(cursor);
      depth += 1;
      cursor = parentBySession.get(cursor);
    }
    return depth;
  };

  const lanes = [...laneMembers.entries()]
    .sort(
      ([left], [right]) =>
        laneDepth(left) - laneDepth(right) || left.localeCompare(right)
    )
    .map(([id, members]) => {
      const milestones = [...members]
        .sort((left, right) => {
          const leftTimestamp = parseTimestamp(left.displayTimestamp) ?? 0;
          const rightTimestamp = parseTimestamp(right.displayTimestamp) ?? 0;
          return (
            leftTimestamp - rightTimestamp || left.id.localeCompare(right.id)
          );
        })
        .map((node) => ({
          ...toDisplayNode(node),
          sequence: turnSequence(node.id),
        }));
      const gaps: StorylineIdleGap[] = [];
      for (let index = 1; index < milestones.length; index += 1) {
        const previous = milestones[index - 1];
        const current = milestones[index];
        const previousTimestamp = parseTimestamp(previous.displayTimestamp);
        const currentTimestamp = parseTimestamp(current.displayTimestamp);
        if (
          previousTimestamp !== null &&
          currentTimestamp !== null &&
          currentTimestamp - previousTimestamp > idleGapMs
        ) {
          gaps.push({
            kind: "idleGap",
            fromTimestamp: previous.displayTimestamp!,
            toTimestamp: current.displayTimestamp!,
            durationMs: currentTimestamp - previousTimestamp,
          });
        }
      }
      const session = sessionNodes.get(id);
      const state =
        id === "unlinked-facts"
          ? "unknown"
          : laneState(session?.lifecycleStatus);
      return {
        id,
        label:
          id === "unlinked-facts"
            ? "Unlinked facts"
            : session?.displayTitle?.trim() ||
              session?.branch?.trim() ||
              titleFromId(id),
        milestones,
        gaps,
        state,
        branch: session?.branch,
        parentLaneId: parentBySession.get(id),
        isActiveTrunk: state === "active" && !childSessions.has(id),
      };
    });

  const connectors = edges
    .filter((edge) =>
      ["forkedFrom", "resumedFrom", "compactedTo", "handoffTo"].includes(
        edge.kind
      )
    )
    .map((edge) => ({
      from: edge.from,
      to: edge.to,
      kind: edge.kind,
      evidenceClass: edge.evidenceClass,
      sourceRef: edge.sourceRef,
    }));
  return {
    lanes,
    connectors,
    unpositioned: unpositioned.sort((left, right) =>
      left.id.localeCompare(right.id)
    ),
  };
}

export function graphToBranchesViewModel(
  graph: JourneyGraphPayload
): BranchesViewModel {
  assertCompleteGraph(graph);
  const ids = new Set(graph.nodes.map((node) => node.id));
  const links = [...graph.edges]
    .sort(compareEdges)
    .filter((edge): edge is JourneyGraphEdge & { kind: BranchLink["kind"] } =>
      ["forkedFrom", "resumedFrom", "compactedTo"].includes(edge.kind)
    )
    .filter((edge) => ids.has(edge.from) && ids.has(edge.to))
    .map((edge) => ({
      from: edge.from,
      to: edge.to,
      kind: edge.kind,
      evidenceClass: edge.evidenceClass,
      sourceRef: edge.sourceRef,
    }));
  const referenced = new Set(links.flatMap((link) => [link.from, link.to]));
  return {
    nodes: [...graph.nodes]
      .filter((node) => referenced.has(node.id))
      .sort(compareNodes)
      .map(toDisplayNode),
    links,
  };
}

export function graphToFileLineageViewModel(
  graph: JourneyGraphPayload
): FileLineageViewModel {
  assertCompleteGraph(graph);
  const byId = new Map(graph.nodes.map((node) => [node.id, node]));
  const links = [...graph.edges]
    .sort(compareEdges)
    .filter(
      (edge): edge is JourneyGraphEdge & { kind: FileLineageLink["kind"] } =>
        ["produced", "modified"].includes(edge.kind)
    )
    .filter(
      (edge) =>
        byId.get(edge.from)?.kind === "file" ||
        byId.get(edge.to)?.kind === "file"
    )
    .map((edge) => ({
      from: edge.from,
      to: edge.to,
      kind: edge.kind,
      evidenceClass: edge.evidenceClass,
      sourceRef: edge.sourceRef,
    }));
  const adjacentIds = new Set(links.flatMap((link) => [link.from, link.to]));
  return {
    files: [...graph.nodes]
      .filter((node) => node.kind === "file" && adjacentIds.has(node.id))
      .sort(compareNodes)
      .map(toDisplayNode),
    adjacentNodes: [...graph.nodes]
      .filter((node) => node.kind !== "file" && adjacentIds.has(node.id))
      .sort(compareNodes)
      .map(toDisplayNode),
    links,
  };
}

function coverageDetail(entry: JourneyCoverage): string {
  if (entry.status === "represented") return "Represented in the Journey graph";
  if (entry.status === "uncovered") return "Uncovered canonical source unit";
  if ("mergedInto" in entry.status)
    return `Merged into: ${entry.status.mergedInto.target}`;
  return `Excluded: ${entry.status.excluded.reason}`;
}

function coverageStatusKind(
  entry: JourneyCoverage
): CoverageLedgerEntry["statusKind"] {
  if (typeof entry.status === "string") return entry.status;
  return "mergedInto" in entry.status ? "mergedInto" : "excluded";
}

export function graphToCoverageLedgerViewModel(
  graph: JourneyGraphPayload
): CoverageLedgerViewModel {
  assertCompleteGraph(graph);
  const summary: CoverageLedgerViewModel["summary"] = {
    represented: 0,
    mergedInto: 0,
    excluded: 0,
    uncovered: 0,
  };
  const entries = [...graph.coverage]
    .sort((left, right) => left.sourceRef.localeCompare(right.sourceRef))
    .map((entry) => {
      const statusKind = coverageStatusKind(entry);
      summary[statusKind] += 1;
      return { ...entry, statusKind, detail: coverageDetail(entry) };
    });
  return { entries, summary, provenanceAudit: "notProvided" };
}
