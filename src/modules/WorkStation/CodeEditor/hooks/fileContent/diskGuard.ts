/**
 * Save-time guard against overwriting a file that changed underneath the
 * editor.
 *
 * ORGII's editor shares a working tree with an agent, so "the file changed
 * since you opened it" is the normal case, not the exotic one: an agent edit,
 * a `git checkout`, or a formatter can all land while a buffer sits open.
 *
 * The branch-switch save path has always compared against disk before
 * writing. ⌘S and the close-tab Save button did not — they called
 * `writeTextFile` directly, so the user's buffer silently replaced whatever
 * had been written since the buffer loaded. This module is the shared guard
 * those paths were missing.
 *
 * The baseline is `originalContent` from `useFileContent`: the bytes read at
 * load, and re-stamped on every successful save. Comparing against it answers
 * exactly "did anything change since the buffer last agreed with disk".
 */
import { readEditorTextFile } from "@src/services/file/editorBoundedIO";
import i18next from "i18next";

import { createLogger } from "@src/hooks/logger";
import { getFileName, toFsPluginPath } from "@src/util/file/pathUtils";

const log = createLogger("fileContent/diskGuard");

/**
 * Whether the bytes on disk still match `baseline`.
 *
 * A failed bounded read rejects: keep the dirty buffer, never interpret an
 * unreadable or oversized file as permission to overwrite it.
 */
export async function isFileUnchangedOnDisk(
  filePath: string,
  baseline: string
): Promise<boolean> {
  try {
    return (await readEditorTextFile(toFsPluginPath(filePath))) === baseline;
  } catch (error) {
    log.warn(
      "[diskGuard] Could not read file to compare before save; preserving dirty buffer",
      error
    );
    throw error;
  }
}

/**
 * Resolve whether a save may proceed, prompting only when disk has diverged.
 *
 * Returns `true` to write, `false` to abort and leave the buffer dirty so the
 * user keeps their edits and can diff or reload deliberately.
 */
export async function confirmSaveOverDiskChanges(
  filePath: string,
  baseline: string
): Promise<boolean> {
  if (await isFileUnchangedOnDisk(filePath, baseline)) return true;

  const { ask } = await import("@tauri-apps/plugin-dialog");
  // `ask` returns a boolean, not the button label — the outcome cannot drift
  // when the labels are translated.
  return ask(
    i18next.t("workstation.fileChangedOnDiskBody", {
      name: getFileName(filePath),
    }),
    {
      title: i18next.t("workstation.fileChangedOnDiskTitle"),
      kind: "warning",
      okLabel: i18next.t("actions.overwrite"),
      cancelLabel: i18next.t("actions.cancel"),
    }
  );
}
