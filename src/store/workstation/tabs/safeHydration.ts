import { assertEditorFileReadable } from "@src/util/file/editorFilePolicy";

import type {
  WorkStationTab,
  WorkstationTabsStateV4,
  WorkstationWorkspaceState,
} from "./types";

function filterWorkspace(
  workspace: WorkstationWorkspaceState,
  allowed: Set<string>
): WorkstationWorkspaceState {
  const tabs = workspace.tabs.filter(
    (tab) => tab.type !== "file" || allowed.has(tab.id)
  );
  const local = new Set(tabs.map((tab) => tab.id));
  const valid = (
    ref: NonNullable<WorkstationWorkspaceState["activeTabRef"]>
  ) =>
    ref.partition === "shared" ? allowed.has(ref.tabId) : local.has(ref.tabId);
  const tabOrder = workspace.tabOrder.filter(valid);
  const activeTabRef =
    workspace.activeTabRef && valid(workspace.activeTabRef)
      ? workspace.activeTabRef
      : (tabOrder[0] ??
        (tabs[0]
          ? { partition: "workspace" as const, tabId: tabs[0].id }
          : null));
  return { ...workspace, tabs, tabOrder, activeTabRef };
}

function filterState(
  state: WorkstationTabsStateV4,
  allowed: Set<string>
): WorkstationTabsStateV4 {
  const map = (workspaces: Record<string, WorkstationWorkspaceState>) =>
    Object.fromEntries(
      Object.entries(workspaces).map(([id, workspace]) => [
        id,
        filterWorkspace(workspace, allowed),
      ])
    );
  return {
    ...state,
    shared: {
      ...state.shared,
      tabs: state.shared.tabs.filter(
        (tab) => tab.type !== "file" || allowed.has(tab.id)
      ),
    },
    globalWorkspace: filterWorkspace(state.globalWorkspace, allowed),
    sessionWorkspaces: map(state.sessionWorkspaces),
    directoryWorkspaces: map(state.directoryWorkspaces ?? {}),
    legacySeed: state.legacySeed
      ? filterWorkspace(state.legacySeed, allowed)
      : null,
  };
}

function allTabs(state: WorkstationTabsStateV4): WorkStationTab[] {
  return [
    ...state.shared.tabs,
    ...state.globalWorkspace.tabs,
    ...Object.values(state.sessionWorkspaces).flatMap((w) => w.tabs),
    ...Object.values(state.directoryWorkspaces ?? {}).flatMap((w) => w.tabs),
    ...(state.legacySeed?.tabs ?? []),
  ];
}

/** Synchronous safe initial state: file models cannot mount before metadata verification. */
export function withoutUnverifiedFileTabs(
  state: WorkstationTabsStateV4
): WorkstationTabsStateV4 {
  return filterState(
    state,
    new Set(
      allTabs(state)
        .filter((tab) => tab.type !== "file")
        .map((tab) => tab.id)
    )
  );
}

/** All scopes, metadata only, four concurrent requests at most. Never reads any body. */
export async function hydrateSafeFileTabs(
  state: WorkstationTabsStateV4
): Promise<WorkstationTabsStateV4> {
  const tabs = allTabs(state);
  const allowed = new Set(
    tabs.filter((tab) => tab.type !== "file").map((tab) => tab.id)
  );
  const files = tabs.filter((tab) => tab.type === "file");
  const decisions = new Map<WorkStationTab, boolean>();
  let next = 0;
  await Promise.all(
    Array.from({ length: Math.min(4, files.length) }, async () => {
      while (next < files.length) {
        const tab = files[next++];
        try {
          if (typeof tab.data?.filePath !== "string")
            throw new Error("Missing file path");
          await assertEditorFileReadable(tab.data.filePath);
          decisions.set(tab, true);
        } catch {
          decisions.set(tab, false);
        }
      }
    })
  );
  // An ID duplicated across partitions must not permit a different unsafe path.
  for (const tab of files)
    if (
      decisions.get(tab) &&
      files.filter((t) => t.id === tab.id).every((t) => decisions.get(t))
    )
      allowed.add(tab.id);
  return filterState(state, allowed);
}
