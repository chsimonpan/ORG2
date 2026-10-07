import { statEditorTextFile as stat } from "@src/services/file/editorBoundedIO";
import { expect, it, vi } from "vitest";

import { writeEditorTextFile as writeTextFile } from "@src/services/file/editorBoundedIO";

import { writeTextFileSerial } from "./writeTextFileSerial";

vi.mock("@src/services/file/editorBoundedIO", () => ({
  statEditorTextFile: vi.fn(async () => ({ isFile: true, size: 8 })),
  readEditorTextFile: vi.fn(async () => "text content"),
  writeEditorTextFile: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-fs", () => ({
  stat: vi.fn(async () => ({ isFile: true, size: 8 })),
}));
it("serializes two owners of the same file, permits other files and releases failed queues", async () => {
  const releases: (() => void)[] = [];
  vi.mocked(writeTextFile).mockImplementation(
    () => new Promise<void>((r) => releases.push(r))
  );
  const a = writeTextFileSerial("/fixture/A", "a");
  const b = writeTextFileSerial("/fixture/A", "b");
  const c = writeTextFileSerial("/fixture/B", "c");
  await vi.waitFor(() => expect(releases).toHaveLength(2));
  expect(vi.mocked(writeTextFile).mock.calls).toEqual([
    ["/fixture/A", "a", undefined],
    ["/fixture/B", "c", undefined],
  ]);
  releases[0]();
  releases[1]();
  await Promise.all([a, c]);
  await vi.waitFor(() => expect(releases).toHaveLength(3));
  expect(writeTextFile).toHaveBeenLastCalledWith("/fixture/A", "b", undefined);
  releases[2]();
  await b;
  vi.mocked(writeTextFile).mockRejectedValueOnce(new Error("failure"));
  await expect(writeTextFileSerial("/fixture/A", "bad")).rejects.toThrow(
    "failure"
  );
  vi.mocked(writeTextFile).mockResolvedValueOnce();
  await writeTextFileSerial("/fixture/A", "retry");
});
it("bounds queued operations including repeated writes to one target", async () => {
  vi.mocked(writeTextFile).mockResolvedValue();
  const saves = Array.from({ length: 128 }, (_, i) =>
    writeTextFileSerial("/fixture/cap", String(i))
  );
  await expect(writeTextFileSerial("/fixture/cap", "overflow")).rejects.toThrow(
    "Too many files"
  );
  await Promise.all(saves);
  await writeTextFileSerial("/fixture/cap", "retry");
});

it("R7-U05 an explicitly new small file is allowed but normal save cannot recreate a deleted target", async () => {
  vi.mocked(stat).mockRejectedValue(new Error("ENOENT"));
  vi.mocked(writeTextFile).mockResolvedValue();
  await expect(
    writeTextFileSerial("/fixture/new", "small", { create: true })
  ).resolves.toBeUndefined();
  await expect(
    writeTextFileSerial("/fixture/deleted", "draft")
  ).rejects.toThrow("ENOENT");
  vi.mocked(stat).mockResolvedValue({ isFile: true, size: 8 } as never);
});
