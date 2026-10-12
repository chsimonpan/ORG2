import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import {
  TaskForkMessageCard,
  parseTaskForkMessage,
} from "./TaskForkMessageCard";

const SUMMARY = [
  "【Task 摘要】来源：Task「对账导出重试」（tf-abc）",
  "子会话：A-task-1",
  "起点：主线消息 m3（序号 3）",
  "结果：完成",
  "摘要模型：anthropic/claude-sonnet-5.5（anthropic）",
  "归并顺序：第 1 个归并（按结束 Task 的先后，结束序 1）",
  "追加位置：主线消息 m3（序号 3）之后",
  "—— 以下为 Task 结论摘要，不是 Task 的完整对话 ——",
  "结论：已实现指数退避。",
].join("\n");

const SEED = [
  "【Task 上下文（压缩）】",
  "来源：主线会话 A，截至消息 m3（序号 3）",
  "Task：对账导出重试（tf-abc）",
  "压缩模型：anthropic/claude-sonnet-5.5（anthropic）",
  "以下是从主线压缩出的上下文，不是原始对话；原始对话不在本会话中。",
  "—— 压缩摘要 ——",
  "目标=账单导出。",
].join("\n");

describe("Task fork message card", () => {
  it("parses the summary envelope into title, meta and body", () => {
    const info = parseTaskForkMessage(SUMMARY)!;
    expect(info.kind).toBe("summary");
    expect(info.title).toBe("Task 摘要 · 对账导出重试");
    expect(info.body).toBe("结论：已实现指数退避。");
    expect(info.meta.map(([k]) => k)).toEqual([
      "来源",
      "子会话",
      "起点",
      "结果",
      "摘要模型",
      "归并顺序",
      "追加位置",
    ]);
  });

  it("parses the seed envelope", () => {
    const info = parseTaskForkMessage(SEED)!;
    expect(info.kind).toBe("seed");
    expect(info.title).toBe("Task 上下文（压缩）· 对账导出重试");
    expect(info.body).toBe("目标=账单导出。");
  });

  it("does not treat ordinary or malformed text as a Task message", () => {
    expect(parseTaskForkMessage("你好")).toBeNull();
    expect(parseTaskForkMessage("【Task 摘要】只有一句话")).toBeNull();
    expect(
      parseTaskForkMessage("【Task 摘要】来源：Task「x」（y）\n—— 以下为 Task 结论摘要 ——\n")
    ).toBeNull();
  });

  it("renders a dedicated, source-labelled card (not a plain user bubble)", () => {
    const html = renderToStaticMarkup(
      createElement(TaskForkMessageCard, { info: parseTaskForkMessage(SUMMARY)! })
    );
    expect(html).toContain('data-testid="task-summary-card"');
    expect(html).toContain("来自 Task（系统归并，非用户输入）");
    expect(html).toContain("结论：已实现指数退避。");
    expect(html).toContain("归并顺序：第 1 个归并");
    const seed = renderToStaticMarkup(
      createElement(TaskForkMessageCard, { info: parseTaskForkMessage(SEED)! })
    );
    expect(seed).toContain('data-testid="task-seed-card"');
  });
});
