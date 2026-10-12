import { createElement } from "react";

const SUMMARY_PREFIX = "【Task 摘要】";
const SEED_PREFIX = "【Task 上下文（压缩）】";
const SUMMARY_SEPARATOR = "—— 以下为 Task 结论摘要";
const SEED_SEPARATOR = "—— 压缩摘要 ——";

export interface TaskForkMessageInfo {
  kind: "summary" | "seed";
  title: string;
  meta: Array<[string, string]>;
  body: string;
}

/** Recognizes the two system-authored Task messages by their fixed envelope (not user text). */
export function parseTaskForkMessage(text: string): TaskForkMessageInfo | null {
  const trimmed = text.trimStart();
  const kind = trimmed.startsWith(SUMMARY_PREFIX)
    ? "summary"
    : trimmed.startsWith(SEED_PREFIX)
      ? "seed"
      : null;
  if (!kind) return null;
  const prefix = kind === "summary" ? SUMMARY_PREFIX : SEED_PREFIX;
  const separator = kind === "summary" ? SUMMARY_SEPARATOR : SEED_SEPARATOR;
  const lines = trimmed.slice(prefix.length).split("\n");
  const cut = lines.findIndex((line) => line.startsWith(separator));
  if (cut < 0) return null;
  const meta: Array<[string, string]> = [];
  for (const line of lines.slice(0, cut)) {
    const at = line.indexOf("：");
    if (at > 0) meta.push([line.slice(0, at), line.slice(at + 1).trim()]);
  }
  const body = lines
    .slice(cut + 1)
    .join("\n")
    .trim();
  if (!body || meta.length === 0) return null;
  const subject = meta.find(([key]) => key === (kind === "summary" ? "来源" : "Task"))?.[1] ?? "";
  const name = /「(.+?)」/.exec(subject)?.[1] ?? subject.split("（")[0] ?? "";
  return {
    kind,
    title: kind === "summary" ? `Task 摘要 · ${name || "Task"}` : `Task 上下文（压缩）· ${name || "Task"}`,
    meta,
    body,
  };
}

/** Dedicated source card: this text came from a Task fork, not from the user. */
export function TaskForkMessageCard({ info }: { info: TaskForkMessageInfo }) {
  const summary = info.kind === "summary";
  return createElement(
    "div",
    {
      className: "flex min-w-0 flex-col gap-1.5",
      "data-testid": summary ? "task-summary-card" : "task-seed-card",
    },
    createElement(
      "div",
      { className: "flex items-center gap-2 py-0.5" },
      createElement(
        "span",
        {
          className:
            "rounded-md bg-fill-3 px-1.5 py-0.5 text-xs font-medium text-text-2",
          "data-testid": "task-card-source",
        },
        summary ? "来自 Task（系统归并，非用户输入）" : "来自主线的压缩上下文（非原始对话）"
      ),
      createElement(
        "span",
        { className: "chat-block-title font-medium text-text-1" },
        info.title
      )
    ),
    createElement(
      "div",
      { className: "allow-select whitespace-pre-wrap break-words text-text-1" },
      info.body
    ),
    createElement(
      "details",
      { className: "text-xs text-text-3", "data-testid": "task-card-meta" },
      createElement("summary", { className: "cursor-pointer" }, "来源详情"),
      createElement(
        "ul",
        { className: "mt-1 flex flex-col gap-0.5" },
        info.meta.map(([key, value]) =>
          createElement("li", { key }, `${key}：${value}`)
        )
      )
    )
  );
}
