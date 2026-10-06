import { invoke } from "@tauri-apps/api/core";

import { toFsPluginPath } from "@src/util/file/pathUtils";

/** This is a native RPC, never a stat/readTextFile fallback. */
export function readEditorTextFile(path: string): Promise<string> {
  return invoke<string>("editor_read_bounded", { path: toFsPluginPath(path) });
}

export function writeEditorTextFile(
  path: string,
  content: string,
  options?: { create?: boolean; expectedContent?: string }
): Promise<void> {
  return invoke<void>("editor_write_bounded", {
    path: toFsPluginPath(path),
    content,
    create: options?.create ?? false,
    expectedContent: options?.expectedContent ?? null,
  });
}
