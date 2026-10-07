// @vitest-environment jsdom
import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
import { statEditorTextFile as stat } from "@src/services/file/editorBoundedIO";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  LARGE_FILE_LIMIT_BYTES,
  assertEditorFileReadable,
  assertEditorFileWritable,
  guardEditorFileOpen,
  utf8ByteLength,
} from "./editorFilePolicy";

vi.mock("@src/services/file/editorBoundedIO", () => ({ statEditorTextFile: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({
  stat: vi.fn(),
  readTextFile: vi.fn(),
  writeTextFile: vi.fn(),
}));
vi.mock("./pathUtils", () => ({ toFsPluginPath: (path: string) => path }));
const metadata = (size: number) => ({ size, isFile: true });
beforeEach(() => vi.clearAllMocks());
describe("R7 shared policy", () => {
  it.each([LARGE_FILE_LIMIT_BYTES - 1, LARGE_FILE_LIMIT_BYTES])(
    "allows boundary %d",
    async (size) => {
      vi.mocked(stat).mockResolvedValue(metadata(size) as never);
      await expect(
        assertEditorFileReadable("/fixture/file")
      ).resolves.toBeUndefined();
    }
  );
  it.each([LARGE_FILE_LIMIT_BYTES + 1, 564 * 1024 * 1024])(
    "blocks metadata %d with zero body IO",
    async (size) => {
      vi.mocked(stat).mockResolvedValue(metadata(size) as never);
      await expect(assertEditorFileReadable("/fixture/large")).rejects.toThrow(
        "/fixture/large"
      );
      expect(readTextFile).not.toHaveBeenCalled();
      expect(writeTextFile).not.toHaveBeenCalled();
    }
  );
  it.each(["ENOENT", "EACCES"])("fails closed on %s", async (error) => {
    vi.mocked(stat).mockRejectedValue(new Error(error));
    await expect(assertEditorFileReadable("/fixture/missing")).rejects.toThrow(
      error
    );
    expect(readTextFile).not.toHaveBeenCalled();
  });
  it("rejects invalid/non-file metadata", async () => {
    vi.mocked(stat).mockResolvedValue({ size: NaN, isFile: false } as never);
    await expect(
      assertEditorFileReadable("/fixture/directory")
    ).rejects.toThrow("metadata");
  });
  it("measures UTF8 and preserves draft when target grows", async () => {
    expect(utf8ByteLength("雨é")).toBe(5);
    vi.mocked(stat).mockResolvedValue(
      metadata(LARGE_FILE_LIMIT_BYTES + 1) as never
    );
    await expect(
      assertEditorFileWritable("/fixture/small", "draft")
    ).rejects.toThrow("Read-only");
    await expect(
      assertEditorFileWritable(
        "/fixture/small",
        "雨".repeat(Math.ceil(LARGE_FILE_LIMIT_BYTES / 3))
      )
    ).rejects.toThrow("limit");
    expect(writeTextFile).not.toHaveBeenCalled();
  });
  it("open action never runs for blocked metadata", async () => {
    vi.mocked(stat).mockResolvedValue(metadata(564 * 1024 * 1024) as never);
    vi.spyOn(window, "alert").mockImplementation(() => {});
    const open = vi.fn();
    expect(await guardEditorFileOpen("/fixture/large", open)).toBe(false);
    expect(open).not.toHaveBeenCalled();
  });
});
