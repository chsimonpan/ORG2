import React, { useEffect, useState } from "react";
import { createRoot } from "react-dom/client";

import { useFileContent } from "../../src/modules/WorkStation/CodeEditor/hooks/fileContent/useFileContent";
import { writeTextFileSerial } from "../../src/services/file/writeTextFileSerial";
import {
  hydrateSafeFileTabs,
  withoutUnverifiedFileTabs,
} from "../../src/store/workstation/tabs/safeHydration";
import type { WorkstationTabsStateV4 } from "../../src/store/workstation/tabs/types";
import { counters } from "./fs-fixture";

const key = "r7-isolated-synthetic-tabs";
const workspace = {
  tabs: [
    {
      id: "large",
      type: "file",
      title: "large",
      data: { filePath: "/fixture/large.txt" },
    },
    {
      id: "small",
      type: "file",
      title: "small",
      data: { filePath: "/fixture/small.txt" },
    },
  ],
  activeTabRef: { partition: "workspace", tabId: "large" },
  tabOrder: ["large", "small"].map((tabId) => ({
    partition: "workspace",
    tabId,
  })),
};
if (!localStorage.getItem(key))
  localStorage.setItem(
    key,
    JSON.stringify({
      version: 4,
      shared: { tabs: [] },
      globalWorkspace: { tabs: [], activeTabRef: null, tabOrder: [] },
      sessionWorkspaces: { fixture: workspace },
      legacySeed: null,
    })
  );
function App() {
  const persisted = JSON.parse(
    localStorage.getItem(key)!
  ) as WorkstationTabsStateV4;
  const [tabs, setTabs] = useState(() => withoutUnverifiedFileTabs(persisted));
  const [path, setPath] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const file = useFileContent({ filePath: path });
  useEffect(() => {
    let active = true;
    void hydrateSafeFileTabs(persisted).then((state) => {
      if (!active) return;
      setTabs(state);
      const w = state.sessionWorkspaces.fixture;
      const tab = w.tabs.find((t) => t.id === w.activeTabRef?.tabId);
      setPath(
        typeof tab?.data.filePath === "string" ? tab.data.filePath : null
      );
    });
    return () => {
      active = false;
    };
  }, []);
  const editable = path && file.contentReady && !file.error && !file.isBinary;
  useEffect(() => {
    if (editable) counters.model++;
  }, [editable]);
  (window as any).r7 = {
    counters,
    tabs,
    path,
    error: file.error,
    content: file.content,
  };
  return (
    <main>
      <h1>Isolated R7 actual-hook fixture</h1>
      <div id="active">{path}</div>
      <button id="large" onClick={() => setPath("/fixture/large.txt")}>
        Select blocked fixture
      </button>
      <button id="small" onClick={() => setPath("/fixture/small.txt")}>
        Select small fixture
      </button>
      {file.error ? (
        <pre id="blocked">{file.error.message}</pre>
      ) : editable ? (
        <textarea
          id="editor"
          value={file.content}
          onChange={(event) =>
            file.updateContent(event.target.value, { type: "human" })
          }
        />
      ) : (
        <p id="loading">Verifying metadata</p>
      )}
      <button
        id="save"
        disabled={!editable || !file.hasUnsavedChanges}
        onClick={async () => {
          await writeTextFileSerial(path!, file.content);
          file.markSaved();
          setSaved(true);
        }}
      >
        Save
      </button>
      {saved && <span id="saved">Saved</span>}
    </main>
  );
}
createRoot(document.getElementById("root")!).render(<App />);
