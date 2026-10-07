import React, { useCallback, useEffect, useRef, useState } from "react";

import {
  type GlobalPathExemption,
  addGlobalPathExemption,
  listGlobalPathExemptions,
  removeGlobalPathExemption,
} from "@src/services/permissions/globalPathExemptions";

/** Explicit user-only grants. No optimistic permission state or filesystem-wide default. */
export default function GlobalPathExemptionsPanel() {
  const [grants, setGrants] = useState<GlobalPathExemption[]>([]);
  const [path, setPath] = useState("");
  const [mode, setMode] = useState<GlobalPathExemption["mode"]>("read");
  const [recursive, setRecursive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const alive = useRef(true);
  const pending = useRef(false);
  const run = useCallback(
    async (operation: () => Promise<GlobalPathExemption[]>) => {
      if (pending.current) return;
      pending.current = true;
      setBusy(true);
      setError("");
      try {
        const rows = await operation();
        if (alive.current) setGrants(rows);
      } catch (failure) {
        if (alive.current) setError(String(failure));
      } finally {
        pending.current = false;
        if (alive.current) setBusy(false);
      }
    },
    []
  );
  useEffect(() => {
    alive.current = true;
    void run(listGlobalPathExemptions);
    return () => {
      alive.current = false;
    };
  }, [run]);
  return (
    <section
      aria-label="Global path exemptions"
      className="flex flex-col gap-2"
    >
      <h3>全局路径授权 / Global path exemptions</h3>
      <p>
        只跨会话共享明确授权；禁止策略仍优先。Shell 工作目录授权不是命令沙箱。
      </p>
      {error && <p role="alert">{error}</p>}
      <form
        onSubmit={(event) => {
          event.preventDefault();
          void run(() => addGlobalPathExemption({ path, mode, recursive }));
        }}
      >
        <input
          aria-label="Absolute grant path"
          required
          value={path}
          disabled={busy}
          onChange={(event) => setPath(event.target.value)}
        />
        <select
          aria-label="Grant access"
          value={mode}
          disabled={busy}
          onChange={(event) =>
            setMode(event.target.value as GlobalPathExemption["mode"])
          }
        >
          <option value="read">只读 / Read</option>
          <option value="read_write">读写 / Read + write</option>
        </select>
        <label>
          <input
            type="checkbox"
            checked={recursive}
            disabled={busy}
            onChange={(event) => setRecursive(event.target.checked)}
          />
          递归 / Recursive
        </label>
        <button type="submit" disabled={busy || !path.trim()}>
          添加 / Add
        </button>
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(listGlobalPathExemptions)}
        >
          刷新 / Refresh
        </button>
      </form>
      <ul>
        {grants.map((grant) => (
          <li key={grant.path}>
            <span>
              {grant.path} — {grant.mode} —{" "}
              {grant.recursive ? "recursive" : "exact"}
            </span>
            <button
              type="button"
              disabled={busy}
              aria-label={`Revoke ${grant.path}`}
              onClick={() =>
                void run(() => removeGlobalPathExemption(grant.path))
              }
            >
              撤销 / Revoke
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
