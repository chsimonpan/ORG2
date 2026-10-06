import { invoke } from "@tauri-apps/api/core";
import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";

import { LARGE_FILE_LIMIT_BYTES } from "@src/util/file/editorFilePolicy";

import { readEditorTextFile, writeEditorTextFile } from "./editorBoundedIO";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
describe("R7 native editor boundary", () => {
  it("registers actual endpoints and uses the same exact budget", () => {
    const rust = readFileSync(
      "src-tauri/src/editor_file_io/bounded.rs",
      "utf8"
    );
    expect(rust).toContain("LARGE_FILE_LIMIT_BYTES: u64 = 10 * 1024 * 1024");
    expect(LARGE_FILE_LIMIT_BYTES).toBe(10 * 1024 * 1024);
    const handlers = readFileSync(
      "src-tauri/src/commands/handler_list.inc",
      "utf8"
    );
    expect(readFileSync("src-tauri/src/app/builder.rs", "utf8")).toContain(
      "editor_file_io"
    );
    expect(handlers).toContain("editor_file_io::editor_read_bounded,");
    expect(handlers).toContain("editor_file_io::editor_write_bounded,");
  });
  it("returns real native results and fails without fallback", async () => {
    vi.mocked(invoke).mockResolvedValueOnce("real body");
    await expect(readEditorTextFile("/fixture/small")).resolves.toBe(
      "real body"
    );
    expect(invoke).toHaveBeenLastCalledWith("editor_read_bounded", {
      path: "/fixture/small",
    });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("too_large"));
    await expect(readEditorTextFile("/fixture/huge")).rejects.toThrow(
      "too_large"
    );
  });
  it("forwards creation intent and loaded-content proof; fails visibly", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(undefined);
    await writeEditorTextFile("/fixture/small", "", { expectedContent: "old" });
    expect(invoke).toHaveBeenLastCalledWith("editor_write_bounded", {
      path: "/fixture/small",
      content: "",
      create: false,
      expectedContent: "old",
    });
    vi.mocked(invoke).mockRejectedValueOnce(new Error("conflict"));
    await expect(writeEditorTextFile("/fixture/small", "new")).rejects.toThrow(
      "conflict"
    );
  });
});
