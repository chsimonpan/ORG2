import { statEditorTextFile as stat } from "@src/services/file/editorBoundedIO";

import { toFsPluginPath } from "./pathUtils";

/** Shared editor budget; exactly 10 MiB is allowed. */
export const LARGE_FILE_LIMIT_BYTES = 10 * 1024 * 1024;
export const utf8ByteLength = (content: string): number =>
  new TextEncoder().encode(content).byteLength;

export class EditorFileBlockedError extends Error {
  readonly type = "too_large";
  constructor(
    readonly path: string,
    readonly size: number
  ) {
    const quoted = "'" + path.replace(/'/g, "'\\''") + "'";
    super(
      `Read-only: ${path}\nSize: ${size} bytes; editor limit: ${LARGE_FILE_LIMIT_BYTES} bytes (10 MiB).\nView in a terminal: head -n 100 -- ${quoted} or less -- ${quoted}`
    );
    this.name = "EditorFileBlockedError";
  }
}

/** Fail closed: never read body to discover size or recover failed metadata. */
export async function assertEditorFileReadable(path: string): Promise<void> {
  const metadata = await stat(toFsPluginPath(path));
  if (
    !metadata.isFile ||
    !Number.isSafeInteger(metadata.size) ||
    metadata.size < 0
  )
    throw new Error(
      `Cannot open editor file: invalid/non-file metadata: ${path}`
    );
  if (metadata.size > LARGE_FILE_LIMIT_BYTES)
    throw new EditorFileBlockedError(path, metadata.size);
}

export async function assertEditorFileWritable(
  path: string,
  content: string,
  allowMissing = false
): Promise<void> {
  const size = utf8ByteLength(content);
  if (size > LARGE_FILE_LIMIT_BYTES)
    throw new EditorFileBlockedError(path, size);
  try {
    await assertEditorFileReadable(path);
  } catch (error) {
    if (!allowMissing || !/ENOENT|no such file|not found/i.test(String(error)))
      throw error;
  }
}

export async function guardEditorFileOpen(
  path: string,
  open: () => void
): Promise<boolean> {
  try {
    await assertEditorFileReadable(path);
    open();
    return true;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    if (typeof window !== "undefined" && typeof window.alert === "function")
      window.alert(message);
    return false;
  }
}
