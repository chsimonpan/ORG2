import type {
  StorylineConnector,
  StorylineLane,
  StorylineMilestone,
  StorylineViewModel,
} from "./viewModel";

/**
 * Pure layout for the Storyline timeline. This module only positions facts
 * (nodes with explicit display timestamps, factual edges); it never infers
 * timing, lanes, or hand-offs that the graph did not record.
 *
 * Visual language absorbed from context-visualizer (upstream): compressed
 * real-time x-axis, agent/session lanes, structural milestone markers that
 * survive label throttling, idle compression bands, fork/resume/compaction
 * connectors drawn as curves, and fade-out tails for stale lanes.
 */

/** Any inter-milestone gap counts at least this much (keeps bursts apart). */
export const TIMELINE_FLOOR_MS = 2 * 60 * 1000;
/** Any inter-milestone gap counts at most this much (long idle is compressed). */
export const TIMELINE_CAP_MS = 25 * 60 * 1000;
/** Gaps above this threshold get a labeled compression band. */
export const TIMELINE_IDLE_GAP_MS = 15 * 60 * 1000;
/** Node kinds that are placed on the timeline lanes. */
export const TIMELINE_KINDS = new Set([
  "session",
  "turn",
  "checkpoint",
  "artifact",
  "commit",
]);
/** Node kinds whose labels are always shown, even when throttled. */
export const STRUCTURAL_KINDS = new Set(["checkpoint", "commit", "artifact"]);
/** Edge kinds that are drawn as connector curves between placed milestones. */
export const CURVE_KINDS = new Set([
  "forkedFrom",
  "resumedFrom",
  "compactedTo",
]);
/** Lane whose members could not be attributed to a session. */
export const UNLINKED_LANE_ID = "unlinked-facts";

export const LANE_HEIGHT = 76;
export const LANE_LABEL_WIDTH = 176;
export const PAD_LEFT = 16;
export const PAD_RIGHT = 32;
export const PAD_TOP = 44;
/** Width of the compressed axis in px. */
export const CONTENT_WIDTH = 1200;
/** Minimum px gap between two visible turn labels inside one lane. */
export const MIN_LABEL_GAP_PX = 104;
/** A lane whose last milestone is older than this fades out at the tail. */
export const FADE_TAIL_MS = 90 * 60 * 1000;

export const MIN_TIMELINE_SCALE = 0.5;
export const MAX_TIMELINE_SCALE = 2.5;

export function clampTimelineScale(scale: number): number {
  return Math.min(MAX_TIMELINE_SCALE, Math.max(MIN_TIMELINE_SCALE, scale));
}

export function zoomTimelineScale(scale: number, delta: number): number {
  return clampTimelineScale(scale * (delta > 0 ? 1.1 : 0.9));
}

/** Fit the recorded canvas to a viewport without changing fact coordinates. */
export function fitTimelineScale(
  viewportWidth: number,
  contentWidth: number
): number {
  if (
    !Number.isFinite(viewportWidth) ||
    !Number.isFinite(contentWidth) ||
    contentWidth <= 0
  )
    return 1;
  return clampTimelineScale(viewportWidth / contentWidth);
}

export function parseTimestampMs(
  value: string | null | undefined
): number | null {
  if (!value) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

export interface CompressedPoint {
  raw: number;
  comp: number;
}

export interface CompressedAxis {
  points: CompressedPoint[];
  compTotal: number;
  pxPerComp: number;
  /** Compressed x for a raw timestamp (clamped to the axis range). */
  xOf: (ts: number) => number;
  /** Intervals longer than TIMELINE_IDLE_GAP_MS, in compressed coordinates. */
  idleBands: Array<{ fromComp: number; toComp: number; ms: number }>;
}

export function buildCompressedAxis(timestamps: number[]): CompressedAxis {
  const sorted = [...new Set(timestamps)].sort((a, b) => a - b);
  if (sorted.length === 0) {
    return {
      points: [],
      compTotal: 0,
      pxPerComp: 1,
      xOf: () => 0,
      idleBands: [],
    };
  }
  const points: CompressedPoint[] = [{ raw: sorted[0], comp: 0 }];
  const idleBands: CompressedAxis["idleBands"] = [];
  for (let i = 1; i < sorted.length; i += 1) {
    const raw = sorted[i] - sorted[i - 1];
    const eff = Math.min(Math.max(raw, TIMELINE_FLOOR_MS), TIMELINE_CAP_MS);
    const comp = points[i - 1].comp + eff;
    if (raw > TIMELINE_IDLE_GAP_MS) {
      idleBands.push({ fromComp: points[i - 1].comp, toComp: comp, ms: raw });
    }
    points.push({ raw: sorted[i], comp });
  }
  const compTotal = points[points.length - 1].comp;
  const pxPerComp = compTotal > 0 ? CONTENT_WIDTH / compTotal : 1;
  const xOf = (ts: number): number => {
    if (ts <= points[0].raw) return points[0].comp * pxPerComp;
    const last = points[points.length - 1];
    if (ts >= last.raw) return last.comp * pxPerComp;
    for (let i = 1; i < points.length; i += 1) {
      if (ts <= points[i].raw) {
        const a = points[i - 1];
        const b = points[i];
        const fraction = (ts - a.raw) / (b.raw - a.raw || 1);
        return (a.comp + fraction * (b.comp - a.comp)) * pxPerComp;
      }
    }
    return last.comp * pxPerComp;
  };
  return { points, compTotal, pxPerComp, xOf, idleBands };
}

export interface PlacedMilestone {
  milestone: StorylineMilestone;
  x: number;
  showLabel: boolean;
}

export interface PlacedLane {
  lane: StorylineLane;
  y: number;
  placed: PlacedMilestone[];
  /** True when the lane is explicitly paused, or stale relative to newest fact. */
  fadeTail: boolean;
  /** Branch lanes get a factual vertical offset from their lineage parent. */
  depth: number;
}

export interface PlacedCurve {
  connector: StorylineConnector;
  path: string;
  fromX: number;
  fromY: number;
  toX: number;
  toY: number;
}

export interface StorylineLayout {
  lanes: PlacedLane[];
  curves: PlacedCurve[];
  /** Factual connectors that could not be drawn (endpoints not placed). */
  uncurved: StorylineConnector[];
  unpositioned: StorylineMilestone[];
  axis: CompressedAxis;
  totalWidth: number;
  totalHeight: number;
  /** Newest raw timestamp across all placed milestones (null when empty). */
  latestTs: number | null;
  /** Compressed x of the injected "now" instant, when it lies on the axis. */
  nowX: number | null;
}

/** Milestone kinds eligible for timeline placement. */
function isTimelineMilestone(milestone: StorylineMilestone): boolean {
  return (
    TIMELINE_KINDS.has(milestone.kind) &&
    parseTimestampMs(milestone.displayTimestamp) !== null
  );
}

export function layoutStoryline(
  viewModel: StorylineViewModel,
  nowMs: number = Date.now()
): StorylineLayout {
  const allTs: number[] = [];
  for (const lane of viewModel.lanes) {
    for (const milestone of lane.milestones) {
      const ts = parseTimestampMs(milestone.displayTimestamp);
      if (isTimelineMilestone(milestone) && ts !== null) allTs.push(ts);
    }
  }
  const axis = buildCompressedAxis(allTs);
  const latestTs = allTs.length > 0 ? Math.max(...allTs) : null;

  const placedById = new Map<string, { x: number; y: number; kind: string }>();
  const laneById = new Map(viewModel.lanes.map((lane) => [lane.id, lane]));
  const depthForLane = (lane: StorylineLane): number => {
    let depth = 0;
    let cursor = lane.parentLaneId;
    const visited = new Set<string>([lane.id]);
    while (cursor && !visited.has(cursor)) {
      visited.add(cursor);
      depth += 1;
      cursor = laneById.get(cursor)?.parentLaneId;
    }
    return depth;
  };
  const lanes: PlacedLane[] = viewModel.lanes.map((lane, index) => {
    const depth = depthForLane(lane);
    const y = PAD_TOP + index * LANE_HEIGHT;
    let lastLabelX = -Infinity;
    const placed = lane.milestones
      .filter(isTimelineMilestone)
      .map((milestone) => {
        const x =
          PAD_LEFT + axis.xOf(parseTimestampMs(milestone.displayTimestamp)!);
        const structural = STRUCTURAL_KINDS.has(milestone.kind);
        const showLabel = structural || x - lastLabelX >= MIN_LABEL_GAP_PX;
        if (showLabel) lastLabelX = x;
        placedById.set(milestone.id, { x, y, kind: milestone.kind });
        return { milestone, x, showLabel };
      });
    const lastTs =
      placed.length > 0
        ? parseTimestampMs(placed[placed.length - 1].milestone.displayTimestamp)
        : null;
    const fadeTail =
      lane.state === "paused" ||
      (latestTs !== null &&
        lastTs !== null &&
        latestTs - lastTs > FADE_TAIL_MS);
    return { lane, y, placed, fadeTail, depth };
  });

  const curves: PlacedCurve[] = [];
  const uncurved: StorylineConnector[] = [];
  for (const connector of viewModel.connectors) {
    if (!CURVE_KINDS.has(connector.kind)) {
      uncurved.push(connector);
      continue;
    }
    const from = placedById.get(connector.from);
    const to = placedById.get(connector.to);
    if (!from || !to) {
      uncurved.push(connector);
      continue;
    }
    const fromX = from.x;
    const fromY = from.y;
    const toX = to.x;
    const toY = to.y;
    const yGap = Math.max(14, Math.abs(toY - fromY) / 2);
    const path = `M ${fromX} ${fromY + 7} C ${fromX} ${fromY + yGap}, ${toX} ${toY - yGap}, ${toX} ${toY - 7}`;
    curves.push({ connector, path, fromX, fromY, toX, toY });
  }

  const totalWidth =
    LANE_LABEL_WIDTH +
    PAD_LEFT +
    Math.max(axis.compTotal * axis.pxPerComp, 160) +
    PAD_RIGHT;
  const totalHeight =
    PAD_TOP + Math.max(viewModel.lanes.length, 1) * LANE_HEIGHT + 24;
  const nowX = nowMarkerX(axis, latestTs, nowMs);
  return {
    lanes,
    curves,
    uncurved,
    unpositioned: viewModel.unpositioned,
    axis,
    totalWidth,
    totalHeight,
    latestTs,
    nowX,
  };
}

/**
 * Compressed x for the "now" instant. It is only drawn while it actually
 * lies on the recorded axis: before the first fact or more than a small
 * lookahead past the newest fact the marker would be a fabricated position.
 * The lookahead is presentation-only; it never changes fact positions.
 */
export function nowMarkerX(
  axis: CompressedAxis,
  latestTs: number | null,
  nowMs: number
): number | null {
  if (axis.points.length === 0 || latestTs === null) return null;
  const firstRaw = axis.points[0].raw;
  const nowLookaheadMs = 2 * 60 * 60 * 1000;
  if (nowMs < firstRaw || nowMs > latestTs + nowLookaheadMs) return null;
  return PAD_LEFT + axis.xOf(nowMs);
}

/** Formats a raw ms timestamp for tick labels (compact local time). */
export function formatTick(ts: number): string {
  const date = new Date(ts);
  const p2 = (n: number) => String(n).padStart(2, "0");
  return `${date.getMonth() + 1}/${date.getDate()} ${p2(date.getHours())}:${p2(date.getMinutes())}`;
}

/** Formats an idle duration as "45m" / "3h" / "1d 2h". */
export function formatDuration(ms: number): string {
  const minutes = Math.round(ms / 60000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h`;
  return `${Math.floor(hours / 24)}d ${hours % 24}h`;
}
