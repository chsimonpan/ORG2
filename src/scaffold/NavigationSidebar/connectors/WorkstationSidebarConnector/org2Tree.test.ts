import { describe, expect, it } from "vitest";

import {
  buildOrg2SessionRoutingMap,
  buildOrg2TreeItems,
  formatOrg2ProjectLabel,
  resolveOrg2SessionStatusTone,
} from "./index";
import { isOrg2WorkspaceSessionLeaf } from "./sidebarConnector.menuItemRouting";

describe("buildOrg2TreeItems", () => {
  it("按 workspace→project→session 归组，work item 仅作为元数据且拒绝 slug 推断", () => {
    const tree = buildOrg2TreeItems([
      {
        session_id: "s1",
        name: "S1",
        projectSlug: "proj-a",
        workItemId: "T-1",
        status: "failed",
      },
      {
        session_id: "s2",
        name: "S2",
        projectSlug: "proj-a",
        workItemId: "T-2",
        status: "running",
      },
      { session_id: "s3", name: "S3", status: "completed" },
    ] as never);
    const workspace = tree[0];
    expect(workspace.label).toBe("工作区层级");
    const unlinked = workspace.children?.find(
      (item) => item.label === "未绑定项目（拒绝推断）"
    );
    expect(unlinked).toBeTruthy();
    expect(unlinked?.children?.map((item) => item.label)).toEqual([
      "S1",
      "S2",
      "S3",
    ]);
    expect(unlinked?.children?.map((item) => item.shortcut)).toEqual([
      "工作项：T-1",
      "工作项：T-2",
      "session",
    ]);
    expect(unlinked?.treeDepth).toBe(1);
    expect(unlinked?.children?.map((item) => item.treeDepth)).toEqual([
      2, 2, 2,
    ]);
    expect(unlinked?.children?.map((item) => item.trailingElement)).toEqual([
      expect.anything(),
      expect.anything(),
      expect.anything(),
    ]);
  });

  it("移除项目 label 的 proj 前缀，但保留普通名称", () => {
    expect(formatOrg2ProjectLabel("proj-a")).toBe("a");
    expect(formatOrg2ProjectLabel("PROJ: Core")).toBe("Core");
    expect(formatOrg2ProjectLabel("project-x")).toBe("project-x");
  });

  it("严格由 Session.status 映射红色错误、蓝色运行、绿色结束状态", () => {
    expect(resolveOrg2SessionStatusTone("failed")).toBe("error");
    expect(resolveOrg2SessionStatusTone("cancelled")).toBe("error");
    expect(resolveOrg2SessionStatusTone("completed")).toBe("ended");
    expect(resolveOrg2SessionStatusTone("running")).toBe("running");
    expect(resolveOrg2SessionStatusTone(undefined)).toBe("running");
  });
});

describe("workspace hierarchy session routing map", () => {
  it("includes sessions outside the visible paginated roster", () => {
    const visible = {
      session_id: "weather",
      projectId: "proj-weather",
    } as never;
    const older = {
      session_id: "ppcharge",
      projectId: "proj-ppcharge",
    } as never;
    const map = buildOrg2SessionRoutingMap(new Map([["weather", visible]]), [
      visible,
      older,
    ]);
    expect(map.get("ppcharge")).toBe(older);
  });
});

describe("workspace hierarchy session routing", () => {
  it("marks only projected session leaves for bypassing project routing", () => {
    expect(
      isOrg2WorkspaceSessionLeaf({
        id: "ppcharge-aug",
        key: "org2-tree-session-ppcharge-aug",
        label: "ppcharge-aug",
      })
    ).toBe(true);
    expect(
      isOrg2WorkspaceSessionLeaf({
        id: "org2-tree-project-proj-ppcharge",
        key: "org2-tree-project-proj-ppcharge",
        label: "ppcharge",
      })
    ).toBe(false);
  });
});
