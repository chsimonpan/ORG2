import React, { useEffect, useRef, useState } from "react";

import {
  LANE_HEIGHT,
  LANE_LABEL_WIDTH,
  PAD_LEFT,
  PAD_TOP,
  fitTimelineScale,
  formatDuration,
  formatTick,
  layoutStoryline,
  zoomTimelineScale,
} from "../timelineLayout";
import type { StorylineMilestone, StorylineViewModel } from "../viewModel";
import { EvidenceSource } from "./EvidenceSource";

/** Marker color per node kind (dark-theme harmonic palette). */
const KIND_COLOR: Record<string, string> = {
  session: "#5EB7E6",
  turn: "#8BA3FF",
  checkpoint: "#55C6B1",
  artifact: "#E2B357",
  commit: "#6FC98B",
};
const CURVE_COLOR: Record<string, string> = {
  forkedFrom: "#C79BF2",
  resumedFrom: "#EC8FB0",
  compactedTo: "#A9C05C",
};
const CURVE_DASH: Record<string, string> = {
  forkedFrom: "",
  resumedFrom: "5 4",
  compactedTo: "2 4",
};

const CARD_WIDTH = 156;
const CARD_HEIGHT = 52;
const LANE_STATE_STYLE: Record<
  string,
  { stroke: string; opacity: number; width: number; dash?: string }
> = {
  active: { stroke: "#57B5E8", opacity: 0.95, width: 3 },
  paused: { stroke: "#B08ACB", opacity: 0.42, width: 1.6, dash: "5 5" },
  completed: { stroke: "#6FC98B", opacity: 0.58, width: 2 },
  unknown: { stroke: "currentColor", opacity: 0.22, width: 1.4 },
};

/** Depth-tinted lane band hues: trunk, first branch, deeper branches. */
const DEPTH_HUE: Record<number, string> = {
  0: "#57B5E8",
  1: "#C79BF2",
  2: "#6FC98B",
};

function markerShape(kind: string, x: number, y: number, color: string) {
  if (kind === "checkpoint")
    return (
      <rect
        x={x - 4}
        y={y - 4}
        width={8}
        height={8}
        rx={1}
        fill={color}
        stroke="currentColor"
        strokeOpacity={0.35}
        strokeWidth={1}
      />
    );
  if (kind === "commit")
    return (
      <path
        d={`M ${x} ${y - 5} L ${x + 5} ${y} L ${x} ${y + 5} L ${x - 5} ${y} Z`}
        fill={color}
        stroke="currentColor"
        strokeOpacity={0.35}
        strokeWidth={1}
      />
    );
  if (kind === "artifact")
    return (
      <path
        d={`M ${x - 5} ${y + 4} L ${x} ${y - 5} L ${x + 5} ${y + 4} Z`}
        fill={color}
        stroke="currentColor"
        strokeOpacity={0.35}
        strokeWidth={1}
      />
    );
  return (
    <circle
      cx={x}
      cy={y}
      r={kind === "session" ? 5 : 4}
      fill={color}
      stroke="currentColor"
      strokeOpacity={0.35}
      strokeWidth={1}
    />
  );
}

export interface InspectorField {
  label: string;
  value: string;
}

/** Pure projection of the facts shown in the hover inspector. */
export function inspectorFields(
  milestone: StorylineMilestone
): InspectorField[] {
  const fields: InspectorField[] = [{ label: "kind", value: milestone.kind }];
  if (milestone.displayTimestamp)
    fields.push({ label: "time", value: milestone.displayTimestamp });
  if (milestone.lifecycleStatus)
    fields.push({ label: "lifecycle", value: milestone.lifecycleStatus });
  if (milestone.branch)
    fields.push({ label: "branch", value: milestone.branch });
  if (milestone.sequence !== null)
    fields.push({ label: "turn", value: String(milestone.sequence) });
  if (milestone.resultSummary)
    fields.push({ label: "result", value: milestone.resultSummary });
  return fields;
}

export const StorylineTimeline: React.FC<{ viewModel: StorylineViewModel }> = ({
  viewModel,
}) => {
  const layout = layoutStoryline(viewModel);
  const { axis, lanes, curves, uncurved, unpositioned } = layout;
  const [inspected, setInspected] = useState<{
    milestone: StorylineMilestone;
    x: number;
    y: number;
  } | null>(null);
  const [scale, setScale] = useState(1);
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const dragOrigin = useRef<{
    x: number;
    y: number;
    panX: number;
    panY: number;
  } | null>(null);
  const viewportRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!dragging) return undefined;
    const onMove = (event: PointerEvent) => {
      const origin = dragOrigin.current;
      if (!origin) return;
      setPan({
        x: origin.panX + event.clientX - origin.x,
        y: origin.panY + event.clientY - origin.y,
      });
    };
    const onUp = () => {
      setDragging(false);
      dragOrigin.current = null;
    };
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp, { once: true });
    return () => {
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
    };
  }, [dragging]);

  const resetView = () => {
    setScale(1);
    setPan({ x: 0, y: 0 });
  };
  const fitView = () => {
    const viewportWidth = viewportRef.current?.clientWidth ?? layout.totalWidth;
    setScale(fitTimelineScale(viewportWidth, layout.totalWidth));
    setPan({ x: 0, y: 0 });
  };

  // Sample up to 6 ticks evenly across the compressed axis.
  const tickStep = Math.max(1, Math.ceil(axis.points.length / 6));
  const ticks = axis.points.filter((_, index) => index % tickStep === 0);

  return (
    <section
      aria-label="Storyline timeline"
      className="space-y-4"
      data-testid="storyline-timeline"
    >
      <p className="text-xs text-text-3">
        Real-time x-axis uses display timestamps only; long idle spans are
        compressed and labeled. Solid cyan is an explicit active trunk; faded
        dashed lanes are explicitly paused or stale. Branches and connectors
        only come from canonical lineage edges.
      </p>

      <div
        ref={viewportRef}
        className={`relative overflow-hidden border border-border-2 ${dragging ? "cursor-grabbing" : "cursor-grab"}`}
        data-testid="storyline-viewport"
        onWheel={(event) => {
          event.preventDefault();
          setScale((current) =>
            zoomTimelineScale(current, event.deltaY < 0 ? 1 : -1)
          );
        }}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          dragOrigin.current = {
            x: event.clientX,
            y: event.clientY,
            panX: pan.x,
            panY: pan.y,
          };
          setDragging(true);
        }}
      >
        <div
          className="mb-2 flex items-center justify-end gap-1 px-1 pt-1"
          onPointerDown={(event) => event.stopPropagation()}
        >
          <button
            type="button"
            className="border border-border-2 px-2 py-1 text-xs"
            onClick={() => setScale((current) => zoomTimelineScale(current, 1))}
            aria-label="Zoom in"
          >
            +
          </button>
          <button
            type="button"
            className="border border-border-2 px-2 py-1 text-xs"
            onClick={() =>
              setScale((current) => zoomTimelineScale(current, -1))
            }
            aria-label="Zoom out"
          >
            -
          </button>
          <button
            type="button"
            className="border border-border-2 px-2 py-1 text-xs"
            onClick={fitView}
          >
            Fit
          </button>
          <button
            type="button"
            className="border border-border-2 px-2 py-1 text-xs"
            onClick={resetView}
          >
            Reset
          </button>
          <span
            className="px-1 text-[10px] text-text-3"
            data-testid="storyline-zoom-level"
          >
            {Math.round(scale * 100)}%
          </span>
        </div>
        <div
          className="overflow-hidden"
          style={{ height: layout.totalHeight + 16 }}
        >
          <div
            className="relative origin-top-left"
            style={{
              width: layout.totalWidth,
              height: layout.totalHeight,
              transform: `translate(${pan.x}px, ${pan.y}px) scale(${scale})`,
              transformOrigin: "top left",
            }}
          >
            {/* Lane labels (fixed left column). */}
            {lanes.map(({ lane, y, depth }) => (
              <div
                key={lane.id}
                className={`absolute top-0 truncate pr-2 text-xs font-medium ${lane.isActiveTrunk ? "text-primary-6" : "text-text-2"}`}
                style={{
                  left: Math.min(depth * 12, 36),
                  top: y + LANE_HEIGHT / 2 - 8,
                  width: LANE_LABEL_WIDTH - 8 - Math.min(depth * 12, 36),
                }}
                data-testid="storyline-lane"
                data-lane-state={lane.state ?? "unknown"}
                data-depth={depth}
              >
                {lane.isActiveTrunk ? "● " : depth > 0 ? "↳ " : ""}
                {lane.label}
                {lane.branch ? ` · ${lane.branch}` : ""}
              </div>
            ))}

            {/* SVG layer: idle bands, depth bands, baselines, markers, curves, NOW. */}
            <svg
              className="absolute top-0"
              style={{ left: LANE_LABEL_WIDTH }}
              width={layout.totalWidth - LANE_LABEL_WIDTH}
              height={layout.totalHeight}
              role="img"
              aria-label="Storyline timeline lanes"
            >
              {/* Idle compression bands (global: any gap above the idle threshold). */}
              {axis.idleBands.map((band) => (
                <g
                  key={`${band.fromComp}-${band.toComp}`}
                  data-testid="storyline-idle-gap"
                >
                  <rect
                    x={PAD_LEFT + band.fromComp * axis.pxPerComp}
                    y={0}
                    width={Math.max(
                      2,
                      (band.toComp - band.fromComp) * axis.pxPerComp
                    )}
                    height={layout.totalHeight}
                    fill="#E2B357"
                    fillOpacity={0.04}
                    stroke="#E2B357"
                    strokeOpacity={0.35}
                    strokeDasharray="4 4"
                    rx={3}
                  />
                  <text
                    x={
                      PAD_LEFT +
                      ((band.fromComp + band.toComp) / 2) * axis.pxPerComp
                    }
                    y={PAD_TOP - 26}
                    textAnchor="middle"
                    fontSize={10}
                    fill="#E2B357"
                    fillOpacity={0.9}
                  >
                    idle {formatDuration(band.ms)}
                  </text>
                </g>
              ))}

              {/* Time ticks. */}
              {ticks.map((point) => (
                <g key={point.raw} data-testid="storyline-tick">
                  <line
                    x1={PAD_LEFT + point.comp * axis.pxPerComp}
                    y1={PAD_TOP - 18}
                    x2={PAD_LEFT + point.comp * axis.pxPerComp}
                    y2={layout.totalHeight}
                    stroke="currentColor"
                    strokeOpacity={0.08}
                    strokeWidth={1}
                  />
                  <text
                    x={PAD_LEFT + point.comp * axis.pxPerComp}
                    y={PAD_TOP - 26}
                    fontSize={10}
                    fill="currentColor"
                    fillOpacity={0.55}
                    textAnchor="middle"
                  >
                    {formatTick(point.raw)}
                  </text>
                </g>
              ))}

              {/* Depth-tinted lane bands: trunk vs branch read at a glance. */}
              {lanes.map(({ lane, y, depth }) => {
                const hue = DEPTH_HUE[depth] ?? "#95A7E0";
                return (
                  <rect
                    key={`band-${lane.id}`}
                    x={0}
                    y={y}
                    width={layout.totalWidth - LANE_LABEL_WIDTH}
                    height={LANE_HEIGHT}
                    fill={hue}
                    fillOpacity={lane.id === "unlinked-facts" ? 0 : 0.05}
                    stroke="none"
                    rx={4}
                    data-testid="storyline-lane-band"
                    data-depth={depth}
                  />
                );
              })}

              {/* NOW marker: drawn only when "now" lies on the recorded axis. */}
              {layout.nowX !== null && (
                <g data-testid="storyline-now">
                  <line
                    x1={layout.nowX}
                    y1={PAD_TOP - 18}
                    x2={layout.nowX}
                    y2={layout.totalHeight}
                    stroke="#EC8FB0"
                    strokeOpacity={0.75}
                    strokeWidth={1.4}
                    strokeDasharray="4 4"
                  />
                  <text
                    x={layout.nowX + 4}
                    y={PAD_TOP - 22}
                    fontSize={10}
                    fontWeight={600}
                    fill="#EC8FB0"
                  >
                    now
                  </text>
                </g>
              )}

              {/* Connector curves (factual edges only; endpoints must be placed). */}
              {curves.map(({ connector, path, fromX, fromY, toX, toY }) => (
                <g
                  key={`${connector.kind}-${connector.from}-${connector.to}-${connector.sourceRef}`}
                  data-testid="storyline-curve"
                >
                  <path
                    d={path}
                    fill="none"
                    stroke={CURVE_COLOR[connector.kind] ?? "#95A7E0"}
                    strokeWidth={1.6}
                    strokeDasharray={CURVE_DASH[connector.kind] ?? ""}
                    strokeOpacity={0.85}
                  />
                  <circle
                    cx={toX}
                    cy={toY}
                    r={3}
                    fill={CURVE_COLOR[connector.kind] ?? "#95A7E0"}
                    stroke="none"
                  />
                </g>
              ))}

              {/* Baselines + markers + fade tails per lane. */}
              {lanes.map(({ lane, y, placed, fadeTail }) => {
                const yCenter = y + LANE_HEIGHT / 2;
                const firstX = placed.length > 0 ? placed[0].x : PAD_LEFT;
                const lastX =
                  placed.length > 0
                    ? placed[placed.length - 1].x
                    : PAD_LEFT + 120;
                const laneStyle =
                  LANE_STATE_STYLE[lane.state ?? "unknown"] ??
                  LANE_STATE_STYLE.unknown;
                return (
                  <g
                    key={lane.id}
                    data-testid="storyline-lane-ribbon"
                    data-lane-state={lane.state ?? "unknown"}
                  >
                    {/* Active trunk gets a soft glow so the main line of work stands out. */}
                    {lane.isActiveTrunk && (
                      <line
                        x1={firstX}
                        y1={yCenter}
                        x2={lastX}
                        y2={yCenter}
                        stroke={laneStyle.stroke}
                        strokeOpacity={0.22}
                        strokeWidth={7}
                        strokeLinecap="round"
                      />
                    )}
                    <line
                      x1={firstX}
                      y1={yCenter}
                      x2={lastX}
                      y2={yCenter}
                      stroke={laneStyle.stroke}
                      strokeOpacity={laneStyle.opacity}
                      strokeWidth={laneStyle.width}
                      strokeDasharray={laneStyle.dash ?? ""}
                    />
                    {placed.map(({ milestone, x }) => (
                      <g
                        key={milestone.id}
                        data-testid="storyline-milestone"
                        onMouseEnter={() =>
                          setInspected({ milestone, x, y: yCenter })
                        }
                        onMouseLeave={() => setInspected(null)}
                      >
                        {markerShape(
                          milestone.kind,
                          x,
                          yCenter,
                          KIND_COLOR[milestone.kind] ?? "#95A7E0"
                        )}
                      </g>
                    ))}
                    {fadeTail && placed.length > 0 && (
                      <line
                        x1={lastX}
                        y1={yCenter}
                        x2={Math.min(
                          lastX + 46,
                          layout.totalWidth - LANE_LABEL_WIDTH
                        )}
                        y2={yCenter}
                        stroke={
                          KIND_COLOR[
                            placed[placed.length - 1].milestone.kind
                          ] ?? "#95A7E0"
                        }
                        strokeOpacity={0.22}
                        strokeWidth={1.4}
                        strokeDasharray="3 5"
                        data-testid="storyline-fade-tail"
                      />
                    )}
                  </g>
                );
              })}
            </svg>

            {/* HTML layer: milestone label cards (kept above the SVG). */}
            {lanes.map(({ lane, y, placed }) =>
              placed
                .filter(({ showLabel }) => showLabel)
                .map(({ milestone, x }) => {
                  const yCenter = y + LANE_HEIGHT / 2;
                  const left = Math.min(
                    Math.max(x - CARD_WIDTH / 2, 4),
                    layout.totalWidth - LANE_LABEL_WIDTH - CARD_WIDTH - 4
                  );
                  return (
                    <div
                      key={milestone.id}
                      className="absolute rounded border border-border-2 bg-bg-1 px-2 py-1 text-[10px] leading-tight shadow-sm"
                      style={{
                        left: LANE_LABEL_WIDTH + left,
                        top: yCenter - CARD_HEIGHT - 6,
                        width: CARD_WIDTH,
                        zIndex: 10,
                      }}
                      data-testid="storyline-milestone-card"
                    >
                      <div className="truncate font-medium text-text-1">
                        <span
                          style={{
                            color: KIND_COLOR[milestone.kind] ?? "#95A7E0",
                          }}
                        >
                          ●
                        </span>{" "}
                        {milestone.kind}: {milestone.title}
                      </div>
                      <div className="truncate text-text-3">
                        {milestone.displayTimestamp}
                        {milestone.sequence !== null
                          ? ` · turn ${milestone.sequence}`
                          : ""}
                        {milestone.lifecycleStatus
                          ? ` · ${milestone.lifecycleStatus}`
                          : ""}
                      </div>
                      {milestone.resultSummary && (
                        <div
                          className="mt-0.5 line-clamp-2 text-text-2"
                          data-testid="storyline-turn-result"
                        >
                          {milestone.resultSummary}
                        </div>
                      )}
                      {milestone.evidenceClass === "canonical" &&
                        milestone.topicTags.length > 0 && (
                          <div
                            className="truncate text-text-3"
                            data-testid="storyline-topic-tags"
                          >
                            {milestone.topicTags.join(" · ")}
                          </div>
                        )}
                      <EvidenceSource
                        evidenceClass={milestone.evidenceClass}
                        sourceRef={milestone.sourceRef}
                      />
                    </div>
                  );
                })
            )}

            {/* Hover inspector: full facts for the milestone under the cursor. */}
            {inspected && (
              <div
                className="absolute rounded border border-border-2 bg-bg-1 px-2.5 py-2 text-[11px] shadow-lg"
                style={{
                  left:
                    LANE_LABEL_WIDTH +
                    Math.min(
                      Math.max(inspected.x + 14, 4),
                      layout.totalWidth - LANE_LABEL_WIDTH - 190
                    ),
                  top: Math.max(inspected.y - 16, PAD_TOP - 10),
                  width: 186,
                  zIndex: 30,
                }}
                data-testid="storyline-inspector"
              >
                <div className="font-medium text-text-1">
                  {inspected.milestone.kind}: {inspected.milestone.title}
                </div>
                {inspectorFields(inspected.milestone).map((field) => (
                  <div key={field.label} className="mt-1">
                    <span className="text-text-3">{field.label}: </span>
                    <span className="text-text-1">{field.value}</span>
                  </div>
                ))}
                <div className="mt-1.5">
                  <EvidenceSource
                    evidenceClass={inspected.milestone.evidenceClass}
                    sourceRef={inspected.milestone.sourceRef}
                  />
                </div>
              </div>
            )}
          </div>
        </div>
      </div>

      {/* Legend for the visual grammar. */}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-text-3">
        {["forkedFrom", "resumedFrom", "compactedTo"].map((kind) => (
          <span key={kind} className="inline-flex items-center gap-1">
            <svg width={22} height={6}>
              <line
                x1={0}
                y1={3}
                x2={22}
                y2={3}
                stroke={CURVE_COLOR[kind]}
                strokeWidth={2}
                strokeDasharray={CURVE_DASH[kind]}
              />
            </svg>
            {kind}
          </span>
        ))}
        <span className="inline-flex items-center gap-1">
          <svg width={22} height={6}>
            <line
              x1={0}
              y1={3}
              x2={22}
              y2={3}
              stroke="currentColor"
              strokeOpacity={0.5}
              strokeWidth={2}
              strokeDasharray="3 5"
            />
          </svg>
          paused / stale branch tail
        </span>
        <span className="inline-flex items-center gap-1">
          <svg width={22} height={6}>
            <rect
              x={0}
              y={0}
              width={22}
              height={6}
              rx={2}
              fill="#E2B357"
              fillOpacity={0.25}
              stroke="#E2B357"
              strokeDasharray="4 4"
            />
          </svg>
          idle compression
        </span>
        <span className="inline-flex items-center gap-1">
          <svg width={22} height={6}>
            <line
              x1={0}
              y1={3}
              x2={22}
              y2={3}
              stroke="#EC8FB0"
              strokeWidth={1.4}
              strokeDasharray="4 4"
            />
          </svg>
          now
        </span>
      </div>

      {/* Factual connectors that could not be drawn as curves. */}
      {uncurved.length > 0 && (
        <div className="space-y-1" aria-label="Factual connectors">
          {uncurved.map((connector) => (
            <div
              key={`${connector.kind}-${connector.from}-${connector.to}-${connector.sourceRef}`}
              className="text-xs text-text-2"
              data-testid="storyline-connector"
            >
              {connector.kind}: {connector.from} to {connector.to}{" "}
              <EvidenceSource
                evidenceClass={connector.evidenceClass}
                sourceRef={connector.sourceRef}
              />
            </div>
          ))}
        </div>
      )}

      {/* Facts without a display time stay visible but unpositioned (fail-closed). */}
      {unpositioned.length > 0 && (
        <div className="border border-border-2 p-3">
          <h3 className="text-sm font-medium text-text-1">
            Facts without display time
          </h3>
          {unpositioned.map((item) => (
            <div key={item.id} className="mt-2 text-xs">
              <span>
                {item.kind}: {item.title}{" "}
              </span>
              <EvidenceSource
                evidenceClass={item.evidenceClass}
                sourceRef={item.sourceRef}
              />
            </div>
          ))}
        </div>
      )}
    </section>
  );
};
