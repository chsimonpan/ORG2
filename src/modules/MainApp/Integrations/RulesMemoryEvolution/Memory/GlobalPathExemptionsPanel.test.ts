// @vitest-environment jsdom
import React, { act } from "react";
import { type Root, createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import GlobalPathExemptionsPanel from "./GlobalPathExemptionsPanel";

const api = vi.hoisted(() => ({
  list: vi.fn(),
  add: vi.fn(),
  remove: vi.fn(),
}));
vi.mock("@src/services/permissions/globalPathExemptions", () => ({
  listGlobalPathExemptions: api.list,
  addGlobalPathExemption: api.add,
  removeGlobalPathExemption: api.remove,
}));

let root: Root;
let host: HTMLDivElement;
beforeEach(() => {
  vi.resetAllMocks();
  api.list.mockResolvedValue([]);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});
async function render() {
  await act(async () => {
    root.render(React.createElement(GlobalPathExemptionsPanel));
  });
}
async function enterPath() {
  await act(async () => {
    const input = host.querySelector(
      'input[aria-label="Absolute grant path"]'
    ) as HTMLInputElement;
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value"
    )!.set!.call(input, "/fixture");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function submit() {
  await act(async () => {
    host
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  });
}
describe("explicit global grants UI", () => {
  it("loads, grants read-only non-recursive by default and revokes from backend response", async () => {
    api.add.mockResolvedValue([
      { path: "/fixture", mode: "read", recursive: false },
    ]);
    api.remove.mockResolvedValue([]);
    await render();
    expect(api.list).toHaveBeenCalledOnce();
    await enterPath();
    await submit();
    expect(api.add).toHaveBeenCalledWith({
      path: "/fixture",
      mode: "read",
      recursive: false,
    });
    await act(async () => {
      (
        host.querySelector(
          'button[aria-label="Revoke /fixture"]'
        ) as HTMLButtonElement
      ).click();
    });
    expect(api.remove).toHaveBeenCalledWith("/fixture");
    expect(
      host.querySelector('button[aria-label="Revoke /fixture"]')
    ).toBeNull();
  });
  it("shows failure and never invents a grant", async () => {
    api.add.mockRejectedValue(new Error("store unavailable"));
    await render();
    await enterPath();
    await submit();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain(
      "store unavailable"
    );
    expect(
      host.querySelector('button[aria-label="Revoke /fixture"]')
    ).toBeNull();
  });
  it("allows explicit write and recursive choice without pre-authorizing it", async () => {
    api.add.mockResolvedValue([]);
    await render();
    await enterPath();
    await act(async () => {
      const select = host.querySelector("select")!;
      select.value = "read_write";
      select.dispatchEvent(new Event("change", { bubbles: true }));
      (
        host.querySelector('input[type="checkbox"]') as HTMLInputElement
      ).click();
    });
    await submit();
    expect(api.add).toHaveBeenCalledWith({
      path: "/fixture",
      mode: "read_write",
      recursive: true,
    });
  });
  it("does not permit duplicate mutations while the authoritative reply is pending", async () => {
    let resolve!: (v: unknown) => void;
    api.add.mockReturnValue(
      new Promise((r) => {
        resolve = r;
      })
    );
    await render();
    await enterPath();
    await submit();
    await submit();
    expect(api.add).toHaveBeenCalledOnce();
    await act(async () => resolve([]));
  });
});
