import { readTextFile, stat } from "@tauri-apps/plugin-fs";
import { describe, expect, it, vi } from "vitest";

import {
  hydrateSafeFileTabs,
  withoutUnverifiedFileTabs,
} from "../safeHydration";
import type {
  WorkstationTabsStateV4,
  WorkstationWorkspaceState,
} from "../types";

vi.mock("@src/services/file/editorBoundedIO", () => ({statEditorTextFile: async (path: string) => (await import("@tauri-apps/plugin-fs")).stat(path)}));
vi.mock("@tauri-apps/plugin-fs", () => ({
  stat: vi.fn(),
  readTextFile: vi.fn(),
}));
vi.mock("@src/util/file/pathUtils", () => ({
  toFsPluginPath: (p: string) => p,
}));
function workspace(): WorkstationWorkspaceState {
  return {
    tabs: [
      {
        id: "large",
        type: "file",
        title: "large",
        data: { filePath: "/fixture/large" },
      },
      {
        id: "small",
        type: "file",
        title: "small",
        data: { filePath: "/fixture/small" },
      },
      { id: "settings", type: "agent-config", title: "settings", data: {} },
    ] as never,
    activeTabRef: { partition: "workspace", tabId: "large" },
    tabOrder: ["large", "small", "settings"].map((tabId) => ({
      partition: "workspace",
      tabId,
    })),
  };
}
function state(): WorkstationTabsStateV4 {
  return {
    version: 4,
    shared: { tabs: [] },
    globalWorkspace: workspace(),
    sessionWorkspaces: { a: workspace(), b: workspace() },
    directoryWorkspaces: { "/fixture": workspace() },
    legacySeed: workspace(),
  };
}
describe("R7 storage/hydration", () => {
  it("cannot mount unverified file models initially", () => {
    const safe = withoutUnverifiedFileTabs(state());
    expect(safe.sessionWorkspaces.a.tabs.map((t) => t.id)).toEqual([
      "settings",
    ]);
    expect(safe.sessionWorkspaces.a.activeTabRef?.tabId).toBe("settings");
  });
  it("stats all partitions, drops large/missing tabs, preserves nonfiles and same-workspace active fallback", async () => {
    vi.mocked(stat).mockImplementation(
      async (path) =>
        ({
          isFile: true,
          size: String(path).endsWith("large") ? 564 * 1024 * 1024 : 8,
        }) as never
    );
    const safe = await hydrateSafeFileTabs(state());
    for (const w of [
      safe.globalWorkspace,
      ...Object.values(safe.sessionWorkspaces),
      ...Object.values(safe.directoryWorkspaces!),
      safe.legacySeed!,
    ]) {
      expect(w.tabs.map((t) => t.id)).toEqual(["small", "settings"]);
      expect(w.activeTabRef?.tabId).toBe("small");
      expect(w.tabOrder.map((r) => r.tabId)).toEqual(["small", "settings"]);
    }
    expect(readTextFile).not.toHaveBeenCalled();
  });
  it("missing file leaves safe non-file active and empty workspace remains empty", async () => {
    vi.mocked(stat).mockRejectedValue(new Error("ENOENT"));
    const safe = await hydrateSafeFileTabs(state());
    expect(safe.globalWorkspace.activeTabRef?.tabId).toBe("settings");
  });
});

it("R7-I01 metadata-only hydration bounds concurrency and exposes no unverified file", async () => {
  let running = 0,
    peak = 0;
  const releases: (() => void)[] = [];
  vi.mocked(stat).mockImplementation(
    () =>
      new Promise((resolve) => {
        running++;
        peak = Math.max(peak, running);
        releases.push(() => {
          running--;
          resolve({ isFile: true, size: 8 } as never);
        });
      })
  );
  const input = state();
  const initial = withoutUnverifiedFileTabs(input);
  const result = hydrateSafeFileTabs(input);
  expect(initial.globalWorkspace.tabs.every((tab) => tab.type !== "file")).toBe(
    true
  );
  expect(peak).toBe(4);
  while (releases.length) {
    releases.splice(0).forEach((release) => release());
    await Promise.resolve();
    await Promise.resolve();
  }
  await result;
  expect(peak).toBeLessThanOrEqual(4);
  expect(readTextFile).not.toHaveBeenCalled();
});
