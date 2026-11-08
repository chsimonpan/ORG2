// SVG storyline adapted from MIT context-visualizer bfd2b50d (Zaheer 2026): agent/branch bands, density ribbons,
// fork curves, milestones, idle compression, NOW marker, legend. React/SVG only; no innerHTML, no network, no model.
// Layout rules: x is real (idle-compressed) time only. A fork sits at its durable anchor's real x; review/close records
// have no timestamp and are drawn as order steps on a dashed "非时间刻度" stretch. Labels never overlap (layoutLabels):
// A scheme (topic multi-ribbon) ports MIT context-visualizer index.html render/buildCenterline/ribbonPath/sample
// (Copyright (c) 2026 Zaheer, MIT License): light per-thread colors, thin 3~12px density ribbons, smooth fork curves
// from the parent lane at the fork's real time, small shape nodes, ≤6-char stroked labels, hover fades others to 12%.
// extra rows / small nudges first, then "…" truncation with the full title as tooltip, then tooltip-only.
import { useEffect, useId, useMemo, useRef, useState } from "react";

import {
  TYPE_NAME,
  UNTITLED,
  currentEvent,
  fitText,
  layoutLabels,
  textWidth,
  timeText,
  toStoryline,
} from "./storylineModel";
import type {
  Box,
  DomainLike,
  LabelReq,
  NodeTopic,
  SEvent,
} from "./storylineModel";
import type { DurableAnchor, JourneySnapshot, MarkerKind } from "./types";
import {
  buildTimeline,
  centerPath,
  ribbonPath,
  sample,
  starPath,
} from "./visualAlgorithms";
import type { Point } from "./visualAlgorithms";

const PAD_L = 160,
  PAD_R = 170,
  AXIS_Y = 26,
  MOON_Y = 43,
  TOP = 58,
  STEP = 72,
  SPAN = 1720;
const STRUCT = new Set<MarkerKind>([
  "question",
  "decision",
  "pivot",
  "abandon",
  "verify",
  "handoff",
]);
const KEY = new Set<MarkerKind>([
  "decision",
  "pivot",
  "abandon",
  "verify",
  "handoff",
]);
const INK = "#e7ecf4",
  FAINT = "#6b778c",
  BG = "#0e1117",
  CAPTION = "分叉后仅示顺序 · 非时间刻度";
// gap=lane pitch, rows=label baseline offsets tried in order, perEvent=minimum horizontal room per timestamped event
const VARIANT = {
  inline: {
    gap: 76,
    fs: 10.5,
    rows: [-15, 24, -28, 37],
    perEvent: 40,
    boost: 1,
    maxCh: 6,
  },
  full: {
    gap: 124,
    fs: 11.5,
    rows: [-16, 26, -31, 41, -46, 56],
    perEvent: 56,
    boost: 1.3,
    maxCh: 6,
  },
};
function Mk({ kind, c }: { kind: MarkerKind; c: string }) {
  switch (kind) {
    case "decision":
      return (
        <path
          d="M0,-7 L7,0 L0,7 L-7,0 Z"
          fill={c}
          stroke={BG}
          strokeWidth={2}
        />
      );
    case "pivot":
      return (
        <>
          <circle r={11} fill={c} opacity={0.14} />
          <path
            d={starPath(8, 3.4, 6)}
            fill={c}
            stroke={BG}
            strokeWidth={1.6}
          />
        </>
      );
    case "abandon":
      return (
        <>
          <circle r={8} fill={BG} stroke={c} />
          <path
            d="M-3.4,-3.4 L3.4,3.4 M3.4,-3.4 L-3.4,3.4"
            stroke={c}
            strokeWidth={2}
          />
        </>
      );
    case "artifact":
      return <rect x={-6.5} y={-6.5} width={13} height={13} rx={3} fill={c} />;
    case "finding":
      return (
        <>
          <circle r={6.5} fill={c} />
          <circle r={2.4} fill={BG} />
        </>
      );
    case "question":
      return (
        <>
          <circle r={8} fill={BG} stroke={c} />
          <text textAnchor="middle" y={3.5} fontSize={10} fill={c}>
            ?
          </text>
        </>
      );
    case "verify":
      return (
        <>
          <circle r={8} fill={c} />
          <path
            d="M-3.4,0 L-1,2.6 L3.6,-2.8"
            fill="none"
            stroke={BG}
            strokeWidth={2.1}
          />
        </>
      );
    case "handoff":
      return (
        <>
          <circle r={8} fill={BG} stroke={c} />
          <path
            d="M-3.5,0 H3 M0.5,-2.6 L3.6,0 L0.5,2.6"
            fill="none"
            stroke={c}
            strokeWidth={1.8}
          />
        </>
      );
    case "interrupt":
      return (
        <>
          <path d="M0,-7.5 L7,6 L-7,6 Z" fill={c} />
          <text textAnchor="middle" y={5} fontSize={9} fill={BG}>
            !
          </text>
        </>
      );
  }
}
const fmt = (t: number) => {
  const d = new Date(t),
    p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
};
const sq = (b: Box) =>
  [b.x0, b.y0, b.x1, b.y1].map((v) => v.toFixed(1)).join(",");
export function JourneyStoryline({
  snapshot,
  domain,
  onSelect,
  titles,
  topics,
  boost,
  variant = "inline",
  fitWidth,
}: {
  snapshot: JourneySnapshot;
  domain: DomainLike | null;
  onSelect: (a: DurableAnchor) => void;
  titles?: Record<string, string>;
  topics?: Record<string, NodeTopic>;
  boost?: number;
  variant?: "inline" | "full";
  fitWidth?: number;
}) {
  const V = VARIANT[variant],
    B = boost ?? V.boost;
  const uid = useId().replace(/:/g, ""),
    [zoom, setZoom] = useState(1),
    [sel, setSel] = useState<string | null>(null),
    [labels, setLabels] = useState(true),
    [focus, setFocus] = useState<string | null>(null),
    [hover, setHover] = useState<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null),
    drag = useRef<number | null>(null);
  const m = useMemo(
    () => toStoryline(snapshot, domain, titles, topics),
    [snapshot, domain, titles, topics]
  );
  const cur = useMemo(() => currentEvent(m), [m]);
  const g = useMemo(() => {
    const tl = buildTimeline(m.events.map((e) => e.ts));
    const span = Math.max(0, tl.width - PAD_L - PAD_R);
    const nTimed = m.events.filter((e) => e.seq === undefined).length;
    const k =
      Math.max(
        1,
        (nTimed * V.perEvent) / SPAN,
        fitWidth ? (fitWidth / B - PAD_L - PAD_R) / SPAN : 1
      ) * zoom;
    const X = (t: number) => PAD_L + (tl.at(t) - PAD_L) * k,
      ex = (e: SEvent) => X(e.ts) + (e.seq ?? 0) * STEP;
    const W =
      PAD_L +
      span * k +
      Math.max(
        PAD_R,
        (Math.max(0, ...m.events.map((e) => e.seq ?? 0)) + 1) * STEP + 60
      );
    const PAD_T = TOP + V.gap / 2,
      laneY = (id: string) =>
        PAD_T +
        Math.max(
          0,
          m.threads.findIndex((t) => t.id === id)
        ) *
          V.gap;
    const H = PAD_T + (m.threads.length - 1) * V.gap + V.gap / 2 + 34;
    const BW = (tl.points.at(-1)?.comp ?? 1) / 18 || 1,
      comp = (t: number) => (tl.at(t) - PAD_L) / tl.scale;
    const per = m.threads.map((th) => {
      const evs = m.events.filter((e) => e.thread === th.id),
        cs = evs.map((e) => comp(e.ts));
      return {
        th,
        evs,
        raw: cs.map((ci) =>
          cs.reduce((s, cj) => s + Math.exp(-(((ci - cj) / BW) ** 2)), 0)
        ),
      };
    });
    const gmax = Math.max(
      1e-9,
      ...per.flatMap((p) =>
        p.evs.map((e, i) => (e.seq === undefined ? p.raw[i] : 0))
      )
    );
    const lines = per
      .filter((p) => p.evs.length)
      .map(({ th, evs, raw }) => {
        const y = laneY(th.id),
          pts: Point[] = [];
        evs.forEach((e, i) => {
          const x = ex(e),
            w =
              e.seq === undefined
                ? 3 + Math.max(0, Math.min(1, raw[i] / gmax)) * 9
                : 4,
            l = pts.at(-1);
          if (l && Math.abs(l.x - x) < 0.5) l.w = Math.max(l.w, w);
          else pts.push({ x, y, w });
        });
        if (th.fade) {
          const l = pts[pts.length - 1];
          pts.push({ x: l.x + 40, y, w: Math.max(1.5, l.w * 0.18) });
        }
        if (pts.length < 2)
          pts.push({ x: pts[0].x + 30, y, w: Math.max(1.5, pts[0].w * 0.5) });
        const fork =
          th.parent && evs[0].seq !== undefined
            ? { x: X(evs[0].ts), py: laneY(th.parent), ts: evs[0].ts }
            : null;
        // Topic fork (cv buildCenterline): ribbon leaves the parent lane at the parent's position just before the child's
        // first real event and eases into its own lane — x stays real time, no synthetic offsets.
        const tfork =
          th.parent && evs[0].seq === undefined
            ? { x: ex(evs[0]), py: laneY(th.parent) }
            : null;
        if (tfork && tfork.py !== y) {
          const x0 =
            tfork.x - Math.min(60, Math.max(24, Math.abs(tfork.py - y) * 0.6));
          pts.unshift(
            { x: x0, y: tfork.py, w: 2.5 },
            { x: (x0 + tfork.x) / 2, y: (tfork.py + y) / 2, w: 2.8 }
          );
        }
        return {
          th,
          evs,
          y,
          dense: sample(pts),
          fork,
          tfork,
          untimed: evs.every((e) => e.seq !== undefined),
        };
      });
    const idle = tl.idle.map((gp) => ({
      x1: PAD_L + gp.from * tl.scale * k,
      x2: PAD_L + gp.to * tl.scale * k,
      ms: gp.ms,
    }));
    const ticks: { x: number; text: string }[] = [];
    if (m.events.length) {
      const t0 = m.events[0].ts,
        t1 = Math.max(...m.events.map((e) => e.ts)),
        sp = (t1 - t0) / 3600000,
        st =
          (sp <= 6 ? 1 : sp <= 14 ? 2 : sp <= 30 ? 4 : sp <= 80 ? 12 : 24) *
          3600000;
      let lx = -1e9,
        lday = "";
      for (
        let t = Math.ceil(t0 / st) * st;
        t <= t1 && ticks.length < 40;
        t += st
      ) {
        const x = X(t);
        if (x - lx < 70 || idle.some((i) => x > i.x1 + 1 && x < i.x2 - 1))
          continue;
        const f = fmt(t),
          day = f.slice(0, 5);
        ticks.push({
          x,
          text: day !== lday || st >= 12 * 3600000 ? f : f.slice(6),
        });
        lx = x;
        lday = day;
      }
    }
    const MIN = (span * k) / 26;
    const shown = lines.flatMap((l) => {
      let last = -1e9;
      return l.evs
        .filter((e) => {
          const x = ex(e);
          if (STRUCT.has(e.type) || e.id === cur?.id || x - last >= MIN) {
            last = x;
            return true;
          }
          return false;
        })
        .map((e) => ({ e, l, x: ex(e), y: l.y }));
    });
    const obstacles: Box[] = [
      { x0: 0, y0: 0, x1: W, y1: TOP - 6 },
      { x0: 0, y0: H - 24, x1: W, y1: H },
    ];
    shown.forEach((s) => {
      const r = s.e.id === cur?.id ? 11 : 9;
      obstacles.push({ x0: s.x - r, y0: s.y - r, x1: s.x + r, y1: s.y + r });
    });
    const captions = lines
      .filter((l) => l.fork && l.untimed)
      .map((l) => ({ id: l.th.id, x: l.fork!.x + 12, y: l.y + 19 }));
    lines.forEach((l) => {
      obstacles.push({ x0: 8, y0: l.y - 14, x1: PAD_L - 14, y1: l.y + 18 });
      if (l.fork)
        obstacles.push({
          x0: l.fork.x - 13,
          y0: l.fork.py - 13,
          x1: l.fork.x + 13,
          y1: l.fork.py + 13,
        });
    });
    captions.forEach((c) =>
      obstacles.push({
        x0: c.x - 2,
        y0: c.y - 9,
        x1: c.x + textWidth(CAPTION, 9) + 2,
        y1: c.y + 3,
      })
    );
    const reqs: LabelReq[] = shown.map((s) => ({
      id: s.e.id,
      x: s.x,
      y: s.y,
      text: s.e.title,
      priority:
        s.e.id === cur?.id
          ? 100
          : KEY.has(s.e.type)
            ? 50
            : s.e.title === UNTITLED
              ? 5
              : 30,
    }));
    const placed = new Map(
      layoutLabels(reqs, {
        fontSize: V.fs,
        offsets: V.rows,
        maxWidth: V.fs * V.maxCh,
        obstacles,
        bounds: { x0: 8, x1: W - 8 },
      }).map((p) => [p.id, p])
    );
    return { tl, W, H, X, lines, idle, ticks, shown, placed, captions };
  }, [m, cur, V, B, zoom, fitWidth]);
  const { W, H } = g,
    selEv = m.events.find((e) => e.id === sel),
    curShown = g.shown.find((s) => s.e.id === cur?.id);
  const vis = [...g.placed.values()].filter((p) => !p.hidden).length,
    dim = (id: string) => {
      const f = hover ?? focus;
      return f && f !== id ? 0.12 : 1;
    };
  const pick = (eid: string) => {
    setSel(eid);
    const e = m.events.find((x) => x.id === eid);
    const n = e?.anchorId
      ? snapshot.nodes.find((x) => x.anchor.durableMessageId === e.anchorId)
      : undefined;
    if (n && n.anchor.ownerSessionId === snapshot.ownerSessionId)
      onSelect(n.anchor);
  };
  const toCur = (smooth: boolean) =>
    scroller.current?.querySelector("[data-current]")?.scrollIntoView?.({
      inline: "center",
      block: "nearest",
      behavior: smooth ? "smooth" : "auto",
    });
  useEffect(() => {
    if (variant === "full") toCur(false);
  }, [variant, cur?.id]);
  return (
    <section
      aria-label="Journey storyline"
      data-owner={snapshot.ownerSessionId}
      data-variant={variant}
      style={{ background: BG, color: INK, borderRadius: 8, padding: 8 }}
    >
      <div
        role="toolbar"
        aria-label="Storyline controls"
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 8,
          alignItems: "center",
          fontSize: variant === "full" ? 13 : 12,
        }}
      >
        <button
          type="button"
          aria-label="横向放大"
          onClick={() => setZoom((z) => Math.min(6, z * 1.25))}
        >
          +
        </button>
        <button
          type="button"
          aria-label="横向缩小"
          onClick={() => setZoom((z) => Math.max(0.5, z / 1.25))}
        >
          -
        </button>
        <button
          type="button"
          onClick={() => {
            setZoom(1);
            setFocus(null);
            if (scroller.current) scroller.current.scrollLeft = 0;
          }}
        >
          复位视图
        </button>
        <button type="button" disabled={!curShown} onClick={() => toCur(true)}>
          定位当前
        </button>
        <label>
          <input
            type="checkbox"
            checked={labels}
            onChange={(e) => setLabels(e.target.checked)}
          />
          里程碑标签
        </label>
        <span style={{ color: FAINT }}>
          {m.threads.length} 线程 · {m.events.length} 事件 · 覆盖{" "}
          {m.coverage.shown}/{m.coverage.nodes} · 夜间压缩 {g.tl.idle.length} 段
          · 标签 {vis}/{g.shown.length}
          {vis < g.shown.length ? "（其余悬停/点击标记查看）" : ""}
        </span>
        <span style={{ color: FAINT }}>会话 {snapshot.ownerSessionId}</span>
      </div>
      <div
        ref={scroller}
        style={{ overflowX: "auto", overflowY: "hidden", marginTop: 4 }}
      >
        <svg
          aria-label="Journey storyline ribbons and milestones"
          role="img"
          viewBox={`0 0 ${W} ${H}`}
          style={{
            width: W * B,
            height: H * B,
            display: "block",
            touchAction: "pan-y",
          }}
          onPointerDown={(e) => {
            if (!(e.target as Element).closest?.("[role=button]")) {
              drag.current = e.clientX;
              e.currentTarget.setPointerCapture?.(e.pointerId);
            }
          }}
          onPointerMove={(e) => {
            if (drag.current !== null && scroller.current) {
              scroller.current.scrollLeft -= e.clientX - drag.current;
              drag.current = e.clientX;
            }
          }}
          onPointerUp={() => {
            drag.current = null;
          }}
          onPointerCancel={() => {
            drag.current = null;
          }}
        >
          <defs>
            {g.lines.map(({ th }) => (
              <linearGradient
                key={th.id}
                id={`${uid}-g-${th.id}`}
                x1="0"
                x2="1"
                y1="0"
                y2="0"
              >
                {th.fade ? (
                  <>
                    <stop offset="0" stopColor={th.color} stopOpacity={0.95} />
                    <stop offset=".72" stopColor={th.color} stopOpacity={0.9} />
                    <stop offset="1" stopColor={th.color} stopOpacity={0.06} />
                  </>
                ) : th.glow ? (
                  <>
                    <stop offset="0" stopColor={th.color} stopOpacity={0.78} />
                    <stop offset="1" stopColor="#ffd1e6" stopOpacity={1} />
                  </>
                ) : (
                  <>
                    <stop offset="0" stopColor={th.color} stopOpacity={0.9} />
                    <stop offset="1" stopColor={th.color} stopOpacity={0.85} />
                  </>
                )}
              </linearGradient>
            ))}
          </defs>
          {g.idle.map((gap, i) => {
            const h = Math.round(gap.ms / 3600000),
              txt = h >= 1 ? h + "h" : Math.round(gap.ms / 60000) + "m";
            return (
              <g key={"idle" + i}>
                <rect
                  x={gap.x1}
                  y={MOON_Y + 5}
                  width={Math.max(2, gap.x2 - gap.x1)}
                  height={H - MOON_Y - 30}
                  rx={3}
                  fill="#1b2233"
                  opacity={0.55}
                >
                  <title>{`压缩 ${txt}；时间顺序不变`}</title>
                </rect>
                <text
                  x={(gap.x1 + gap.x2) / 2}
                  y={MOON_Y}
                  textAnchor="middle"
                  fontSize={10}
                  fill={FAINT}
                >
                  {"🌙 " + txt}
                </text>
              </g>
            );
          })}
          {g.ticks.map((t) => (
            <g key={"tick" + t.x}>
              <line
                x1={t.x}
                x2={t.x}
                y1={AXIS_Y + 5}
                y2={H - 28}
                stroke="#fff"
                strokeOpacity={0.05}
              />
              <text
                x={t.x}
                y={AXIS_Y}
                textAnchor="middle"
                fontSize={11}
                fill={FAINT}
              >
                {t.text}
              </text>
            </g>
          ))}
          {curShown && (
            <line
              x1={curShown.x}
              y1={AXIS_Y + 6}
              x2={curShown.x}
              y2={H - 26}
              stroke={curShown.l.th.color}
              strokeWidth={1.5}
              strokeDasharray="4 4"
              strokeOpacity={0.6}
            />
          )}
          {g.lines
            .filter((l) => l.fork)
            .map(({ th, y, fork }) => {
              const f = fork!,
                d = y - f.py;
              return (
                <path
                  key={"conn" + th.id}
                  d={`M${f.x},${f.py + 9} C${f.x + 18},${f.py + d * 0.35} ${f.x + 18},${y - d * 0.35} ${f.x},${y - 9}`}
                  fill="none"
                  stroke={th.color}
                  strokeWidth={3}
                  strokeLinecap="round"
                  strokeOpacity={0.85}
                  opacity={dim(th.id)}
                />
              );
            })}
          {g.lines.map(({ th, dense, untimed }) => (
            <g
              key={th.id}
              data-thread={th.id}
              opacity={dim(th.id)}
              onPointerEnter={() => setHover(th.id)}
              onPointerLeave={() => setHover(null)}
            >
              <path
                d={ribbonPath(dense)}
                fill={`url(#${uid}-g-${th.id})`}
                fillOpacity={untimed ? 0.5 : 0.78}
                stroke="none"
                strokeDasharray={untimed ? "4 3" : undefined}
              />
              <path
                d={centerPath(dense)}
                fill="none"
                stroke={untimed ? INK : th.color}
                strokeWidth={untimed ? 1.1 : 0.9}
                strokeOpacity={untimed ? 0.55 : th.fade ? 0.35 : 0.45}
                strokeDasharray={untimed ? "3 5" : th.fade ? "1 7" : undefined}
              />
            </g>
          ))}
          {g.captions.map((c) => (
            <text
              key={"cap" + c.id}
              x={c.x}
              y={c.y}
              fontSize={9}
              fill={FAINT}
              opacity={dim(c.id)}
            >
              {CAPTION}
            </text>
          ))}
          {g.lines.map(({ th, evs, y }) => {
            const active = th.id === m.activeThread;
            return (
              <g
                key={"l" + th.id}
                role="button"
                aria-label={`聚焦线程 ${th.name}`}
                aria-pressed={focus === th.id}
                style={{ cursor: "pointer" }}
                opacity={dim(th.id)}
                onClick={() => setFocus((f) => (f === th.id ? null : th.id))}
              >
                <title>{th.name}</title>
                <rect
                  x={14}
                  y={y - 6}
                  width={16}
                  height={12}
                  rx={3}
                  fill={th.color}
                />
                <text
                  x={38}
                  y={y + 1}
                  fontSize={13}
                  fontWeight={700}
                  fill={th.color}
                  style={{ paintOrder: "stroke", stroke: BG, strokeWidth: 4 }}
                >
                  {fitText(th.name, PAD_L - 52, 13)}
                </text>
                <text x={38} y={y + 15} fontSize={10} fill={FAINT}>
                  {evs.length} 事件{active ? " · 当前" : ""}
                  {th.fade ? " · 已关闭" : ""}
                </text>
              </g>
            );
          })}
          {g.shown.map(({ e, l, x, y }) => {
            const isCur = e.id === cur?.id;
            return (
              <g
                key={e.id}
                data-event-id={e.id}
                data-current={isCur ? "true" : undefined}
                transform={`translate(${x},${y})`}
                opacity={dim(l.th.id)}
                style={{ cursor: e.anchorId ? "pointer" : "default" }}
                onPointerEnter={() => setHover(l.th.id)}
                onPointerLeave={() => setHover(null)}
                onClick={() => pick(e.id)}
                role="button"
                aria-label={`${TYPE_NAME[e.type]} ${e.title}${isCur ? "（当前）" : ""}`}
              >
                <circle r={16} fill="transparent" />
                {isCur && (
                  <>
                    <circle r={16} fill={l.th.color} opacity={0.2} />
                    <circle
                      r={11}
                      fill="none"
                      stroke={l.th.color}
                      strokeWidth={2}
                    >
                      <animate
                        attributeName="r"
                        values="11;22;11"
                        dur="2.4s"
                        repeatCount="indefinite"
                      />
                      <animate
                        attributeName="opacity"
                        values=".9;0;.9"
                        dur="2.4s"
                        repeatCount="indefinite"
                      />
                    </circle>
                  </>
                )}
                <g transform={isCur ? "scale(.95)" : "scale(.68)"}>
                  <Mk kind={e.type} c={l.th.color} />
                </g>
                {isCur && (
                  <circle
                    r={12.5}
                    fill="none"
                    stroke="#fff"
                    strokeWidth={1.6}
                  />
                )}
                {sel === e.id && (
                  <circle
                    r={15}
                    fill="none"
                    stroke="#fff"
                    strokeWidth={1.5}
                    strokeDasharray="3 2"
                  />
                )}
                <title>{`${TYPE_NAME[e.type]} · ${timeText(e, fmt)}${isCur ? " · 当前" : ""}\n${e.title}\n${e.summary.slice(0, 160)}`}</title>
              </g>
            );
          })}
          {g.lines
            .filter((l) => l.fork)
            .map(({ th, fork }) => (
              <circle
                key={"ring" + th.id}
                cx={fork!.x}
                cy={fork!.py}
                r={12}
                fill="none"
                stroke={th.color}
                strokeWidth={2}
                pointerEvents="stroke"
                opacity={dim(th.id)}
              >
                <title>{`分叉点 · ${th.name} · 锚点 ${fmt(fork!.ts)}（真实消息时间）`}</title>
              </circle>
            ))}
          {labels &&
            g.shown.map(({ e, l, x, y }) => {
              const p = g.placed.get(e.id);
              if (!p || p.hidden || !p.box) return null;
              const isCur = e.id === cur?.id,
                untitled = e.title === UNTITLED,
                up = p.dy < 0;
              const lead =
                Math.abs(p.dy) >
                  Math.max(Math.abs(V.rows[0]), Math.abs(V.rows[1])) ||
                Math.abs(p.x - x) > 2;
              return (
                <g
                  key={"t" + e.id}
                  opacity={dim(l.th.id)}
                  style={{ cursor: "pointer" }}
                  onClick={() => pick(e.id)}
                >
                  {lead && (
                    <line
                      x1={x}
                      y1={y + (up ? -10 : 10)}
                      x2={p.x}
                      y2={up ? p.box.y1 : p.box.y0}
                      stroke={l.th.color}
                      strokeOpacity={0.55}
                      strokeWidth={0.8}
                    />
                  )}
                  <text
                    data-label-for={e.id}
                    data-box={sq(p.box)}
                    x={p.x}
                    y={p.y}
                    textAnchor="middle"
                    fontSize={V.fs}
                    fontWeight={isCur ? 800 : 400}
                    fontStyle={untitled ? "italic" : undefined}
                    fill={isCur ? "#fff" : untitled ? FAINT : INK}
                    style={{
                      paintOrder: "stroke",
                      stroke: BG,
                      strokeWidth: 4,
                      strokeLinejoin: "round",
                    }}
                  >
                    {p.text}
                  </text>
                  {p.truncated && <title>{p.full}</title>}
                </g>
              );
            })}
          {curShown && (
            <text
              x={curShown.x}
              y={12}
              textAnchor="middle"
              fontSize={11}
              fontWeight={700}
              fill={curShown.l.th.color}
            >
              NOW ▾
            </text>
          )}
          <text x={PAD_L} y={H - 8} fontSize={11} fill={FAINT}>
            {m.events.length
              ? fmt(m.events[0].ts) +
                " → " +
                fmt(Math.max(...m.events.map((e) => e.ts)))
              : ""}
          </text>
        </svg>
      </div>
      <div
        aria-label="Storyline legend"
        style={{
          display: "flex",
          flexWrap: "wrap",
          gap: 10,
          fontSize: variant === "full" ? 12 : 11,
          color: FAINT,
          marginTop: 4,
        }}
      >
        {(Object.keys(TYPE_NAME) as MarkerKind[]).map((k) => (
          <span
            key={k}
            style={{ display: "inline-flex", alignItems: "center", gap: 4 }}
          >
            <svg width={20} height={20} viewBox="-10 -10 20 20">
              <Mk kind={k} c="#8BA3FF" />
            </svg>
            {TYPE_NAME[k]}
          </span>
        ))}
        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
          <svg width={20} height={20} viewBox="-10 -10 20 20">
            <circle r={6} fill="none" stroke="#8BA3FF" strokeWidth={2} />
            <circle r={8.5} fill="none" stroke="#fff" strokeWidth={1.2} />
          </svg>
          当前节点
        </span>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
          <svg width={20} height={20} viewBox="-10 -10 20 20">
            <circle r={7} fill="none" stroke="#E2B357" strokeWidth={2} />
          </svg>
          分叉点（锚点真实时间）
        </span>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 4 }}>
          <svg width={26} height={20} viewBox="-13 -10 26 20">
            <line
              x1={-12}
              x2={12}
              y1={0}
              y2={0}
              stroke={INK}
              strokeOpacity={0.6}
              strokeDasharray="3 4"
            />
          </svg>
          {CAPTION}
        </span>
        <span>
          粗细=投入（事件密度）· 横轴=真实时间（夜间压缩）· 淡出=已关闭/暂停 ·
          点击线程名聚焦
        </span>
      </div>
      {selEv && (
        <p role="status" style={{ fontSize: 12, margin: "6px 0 0" }}>
          {TYPE_NAME[selEv.type]} · {timeText(selEv, fmt)} · {selEv.title} ·{" "}
          {selEv.summary.slice(0, 200)}
        </p>
      )}
    </section>
  );
}
