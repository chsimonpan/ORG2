import React from "react";

import type { OptimizedChatItem } from "./chatItemPipeline/types";

export interface ChatMinimapItem {
  flatIndex: number;
  /** 小地图标记唯一序号；active 只能用它判断，不能用可能因折叠映射重合的 flatIndex。 */
  markerIndex: number;
  /** 轮次号；同一轮的 user/agent 标记共用同一个编号。 */
  turnNumber: number;
  /** 兼容旧调用方/测试快照；等同于 turnNumber。 */
  order: number;
  kind: "user" | "agent";
  roleLabel: "user" | "agent";
  timeLabel: string;
  /** 该标记覆盖的展示 flat index 闭区间终点，用于把视口锚点映射回所属轮次段。 */
  endFlatIndex: number;
}

export interface ChatMinimapProps {
  items: readonly ChatMinimapItem[];
  activeMarkerIndex: number;
  onJump: (flatIndex: number) => void;
}

export const ChatMinimap = React.forwardRef<HTMLDivElement, ChatMinimapProps>(
  ({ items, activeMarkerIndex, onJump }, ref) => (
    <div
      ref={ref}
      data-testid="chat-minimap"
      className="absolute right-2 top-4 z-[80] flex max-h-[calc(100%-2rem)] w-14 flex-col items-center overflow-y-auto overflow-x-visible rounded-xl bg-chat-pane/70 py-1 shadow-sm backdrop-blur-sm [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      aria-label="聊天小地图"
      data-minimap-count={items.length}
    >
      {items.map((item) => {
        const isActive = item.markerIndex === activeMarkerIndex;
        return (
          <button
            key={item.markerIndex}
            type="button"
            title={`#${item.turnNumber} · ${item.roleLabel} · ${item.timeLabel}`}
            aria-label={`跳转到第 ${item.turnNumber} 轮 ${item.roleLabel} 消息`}
            data-testid="chat-minimap-marker"
            data-kind={item.kind}
            data-active={isActive ? "true" : "false"}
            data-jump-index={item.flatIndex}
            className="group flex h-[18px] w-full shrink-0 items-center justify-center gap-1 rounded-sm transition-colors hover:bg-fill-2/80"
            onClick={() => onJump(item.flatIndex)}
          >
            <span
              className="block rounded-full transition-all group-hover:h-2.5 group-hover:w-2.5"
              style={{
                height: isActive ? 12 : 7,
                width: isActive ? 12 : 7,
                backgroundColor: isActive
                  ? "#B56B6B"
                  : item.kind === "user"
                    ? "#7A9E9F"
                    : "#C7A27C",
                boxShadow: isActive
                  ? "0 0 0 2px rgba(181, 107, 107, 0.24)"
                  : undefined,
              }}
            />
            {isActive && (
              <span
                data-active-label="true"
                className="min-w-5 whitespace-nowrap text-left text-[9px] font-medium leading-none text-text-2"
              >
                #{item.turnNumber}
              </span>
            )}
          </button>
        );
      })}
    </div>
  )
);

ChatMinimap.displayName = "ChatMinimap";

export function resolveChatMinimapActiveMarkerIndex(
  items: readonly ChatMinimapItem[],
  viewportAnchorFlatIndex: number
): number {
  if (items.length === 0) return -1;
  const anchor = Number.isFinite(viewportAnchorFlatIndex)
    ? Math.max(0, viewportAnchorFlatIndex)
    : (items[items.length - 1]?.endFlatIndex ?? 0);
  // # active 语义：用视口中心线偏下项作为当前上下文，取这条线及以下命中的第一个 user/agent 段。
  // # 返回唯一 markerIndex，避免 user/agent 在折叠映射后 flatIndex 重合时多个红点同时 active。
  const containing = items.find(
    (item) => anchor >= item.flatIndex && anchor <= item.endFlatIndex
  );
  if (containing) return containing.markerIndex;
  const below = items.find((item) => item.flatIndex >= anchor);
  return (below ?? items[items.length - 1]).markerIndex;
}

export function buildChatMinimapJumpRequest(flatIndex: number): {
  index: number;
  align: "center";
  behavior: "smooth";
  targetLineAlign: "center-line";
} {
  return {
    index: flatIndex,
    align: "center",
    behavior: "smooth",
    targetLineAlign: "center-line",
  };
}

export function buildChatMinimapItemsFromSession(
  items: readonly OptimizedChatItem[],
  originalToFlatIndex: ReadonlyMap<number, number>
): ChatMinimapItem[] {
  // # 全局小地图必须反映整个 session 的原始对话序列，而不是当前页/折叠后的 flatItems。
  // # 因此这里从 optimizedChatHistory 扫描所有消息，再用 originalToFlatIndex 映射到可滚动的 flat index。
  const result: ChatMinimapItem[] = [];
  let currentTurn = 0;
  let pendingAgent: ChatMinimapItem | null = null;

  const resolveFlatIndex = (sourceIndex: number): number =>
    originalToFlatIndex.get(sourceIndex) ?? sourceIndex;

  const flushAgent = () => {
    if (!pendingAgent) return;
    result.push(pendingAgent);
    pendingAgent = null;
  };

  for (const [sourceIndex, item] of items.entries()) {
    const kind = getChatMinimapKind(item);
    if (!kind) continue;
    const flatIndex = resolveFlatIndex(sourceIndex);
    if (kind === "user") {
      flushAgent();
      currentTurn += 1;
      result.push(
        createChatMinimapItem({
          flatIndex,
          endFlatIndex: flatIndex,
          turnNumber: currentTurn,
          kind,
          item,
        })
      );
      continue;
    }

    if (currentTurn === 0) continue;
    if (!pendingAgent) {
      pendingAgent = createChatMinimapItem({
        flatIndex,
        endFlatIndex: flatIndex,
        turnNumber: currentTurn,
        kind: "agent",
        item,
      });
    } else {
      pendingAgent.endFlatIndex = Math.max(
        pendingAgent.endFlatIndex,
        flatIndex
      );
    }
  }
  flushAgent();
  return assignChatMinimapMarkerIndices(result);
}

export function buildChatMinimapItems(
  items: readonly OptimizedChatItem[],
  _groupCounts?: readonly number[],
  _groupHeaders?: readonly (OptimizedChatItem | null)[]
): ChatMinimapItem[] {
  // # displayGroupCounts/displayGroupHeaders 是虚拟列表分组/折叠分页语义，不是可靠轮次语义；
  // # 真实长会话可能被合成 1-2 个 display group，按它建小地图会只剩 1-2 个标记。
  // # 因此小地图永远按最终展示消息序列扫描 role：user 开新轮，后续非 user 合并为 agent 段。
  return buildChatMinimapItemsFromRoles(items);
}

function buildChatMinimapItemsFromRoles(
  items: readonly OptimizedChatItem[]
): ChatMinimapItem[] {
  const result: ChatMinimapItem[] = [];
  let currentTurn = 0;
  let pendingAgent: ChatMinimapItem | null = null;

  const flushAgent = () => {
    if (!pendingAgent) return;
    result.push(pendingAgent);
    pendingAgent = null;
  };

  for (const [flatIndex, item] of items.entries()) {
    const kind = getChatMinimapKind(item);
    if (!kind) continue;
    if (kind === "user") {
      flushAgent();
      currentTurn += 1;
      result.push(
        createChatMinimapItem({
          flatIndex,
          endFlatIndex: flatIndex,
          turnNumber: currentTurn,
          kind,
          item,
        })
      );
      continue;
    }

    if (currentTurn === 0) continue;
    if (!pendingAgent) {
      pendingAgent = createChatMinimapItem({
        flatIndex,
        endFlatIndex: flatIndex,
        turnNumber: currentTurn,
        kind: "agent",
        item,
      });
    } else {
      pendingAgent.endFlatIndex = flatIndex;
    }
  }
  flushAgent();
  return assignChatMinimapMarkerIndices(result);
}

function assignChatMinimapMarkerIndices(
  items: ChatMinimapItem[]
): ChatMinimapItem[] {
  return items.map((item, markerIndex) => ({ ...item, markerIndex }));
}

function createChatMinimapItem({
  flatIndex,
  endFlatIndex,
  turnNumber,
  kind,
  item,
}: {
  flatIndex: number;
  endFlatIndex: number;
  turnNumber: number;
  kind: "user" | "agent";
  item: OptimizedChatItem | undefined | null;
}): ChatMinimapItem {
  return {
    flatIndex,
    markerIndex: -1,
    endFlatIndex,
    turnNumber,
    order: turnNumber,
    kind,
    roleLabel: kind,
    // # tooltip 时间：真实 displayFlatItems 可能把时间放在 event/message/顶层任一路径。
    timeLabel: getChatItemTimestamp(item) ?? "—",
  };
}

function getChatMinimapKind(
  item: OptimizedChatItem | undefined
): "user" | "agent" | null {
  const rawRole = getChatItemRole(item);
  if (/user|human/i.test(rawRole)) return "user";
  if (
    /assistant|agent|tool|result|system|function|observation|activity/i.test(
      rawRole
    )
  ) {
    return "agent";
  }
  // # 兼容真实 OptimizedChatItem：工具/活动分组常只有 type/kind，没有 event.role；
  // # 这些都是 user 之后的 agent 段，不能因为 role 缺失把小地图清空。
  const fallbackType = String(
    readFirstString(item, ["type", "kind", "event.actionType"])
  );
  if (fallbackType && !/threadSelector|structural/i.test(fallbackType))
    return "agent";
  return null;
}

function getChatItemRole(item: OptimizedChatItem | undefined): string {
  return String(
    readFirstString(item, [
      "role",
      "source",
      "message.role",
      "message.source",
      "message.type",
      "event.role",
      "event.source",
      "event.actionType",
      "event.type",
      "event.kind",
      "type",
      "kind",
    ]) ?? ""
  );
}


function getChatItemTimestamp(
  item: OptimizedChatItem | null | undefined
): string | undefined {
  return readFirstString(item, [
    "created_at",
    "createdAt",
    "timestamp",
    "message.created_at",
    "message.createdAt",
    "message.timestamp",
    "event.created_at",
    "event.createdAt",
    "event.timestamp",
  ]);
}

function readFirstString(
  value: unknown,
  paths: readonly string[]
): string | undefined {
  for (const path of paths) {
    const found = readPath(value, path);
    if (typeof found === "string" && found.length > 0) return found;
  }
  return undefined;
}

function readPath(value: unknown, path: string): unknown {
  return path.split(".").reduce<unknown>((current, key) => {
    if (!current || typeof current !== "object") return undefined;
    return (current as Record<string, unknown>)[key];
  }, value);
}
