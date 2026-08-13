import { describe, expect, it } from "vitest";


import { buildOrg2TreeItems } from "./index";

describe("buildOrg2TreeItems", () => {
  it("按 workspace→project→session 归组，work item 仅作为元数据且拒绝 slug 推断", () => {
    const tree = buildOrg2TreeItems([
      {
        session_id: "s1",
        name: "S1",
        projectSlug: "proj-a",
        workItemId: "T-1",
      },
      {
        session_id: "s2",
        name: "S2",
        projectSlug: "proj-a",
        workItemId: "T-2",
      },
      { session_id: "s3", name: "S3" },
    ] as never);
    const workspace = tree[0];
    expect(workspace.label).toBe("工作区层级");
    // projectSlug is not an authoritative Journey binding, so these remain
    // explicitly unbound rather than being guessed into proj-a.
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
  });

});
