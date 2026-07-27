#!/usr/bin/env node
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const LEGACY_MARKER = "--session-provenance-hook codex";

function removeLegacyOrgiiCodexHooks(config) {
  if (!config || typeof config !== "object" || !config.hooks || typeof config.hooks !== "object") {
    return false;
  }

  let changed = false;
  for (const [event, groups] of Object.entries(config.hooks)) {
    if (!Array.isArray(groups)) continue;
    const remainingGroups = groups
      .map((group) => {
        if (!group || !Array.isArray(group.hooks)) return group;
        const hooks = group.hooks.filter((hook) => {
          const command = typeof hook?.command === "string" ? hook.command : "";
          const commandWindows = typeof hook?.commandWindows === "string" ? hook.commandWindows : "";
          const legacy = command.includes(LEGACY_MARKER) || commandWindows.includes(LEGACY_MARKER);
          changed ||= legacy;
          return !legacy;
        });
        return hooks.length === group.hooks.length ? group : { ...group, hooks };
      })
      .filter((group) => !Array.isArray(group?.hooks) || group.hooks.length > 0);
    if (remainingGroups.length === 0) delete config.hooks[event];
    else config.hooks[event] = remainingGroups;
  }
  if (Object.keys(config.hooks).length === 0) delete config.hooks;
  return changed;
}

function repairCodexHooks(hooksPath = path.join(os.homedir(), ".codex", "hooks.json")) {
  if (!fs.existsSync(hooksPath)) return { changed: false, hooksPath };
  const raw = fs.readFileSync(hooksPath, "utf8");
  const config = JSON.parse(raw);
  const changed = removeLegacyOrgiiCodexHooks(config);
  if (changed) fs.writeFileSync(hooksPath, `${JSON.stringify(config, null, 2)}\n`);
  return { changed, hooksPath };
}

if (require.main === module) {
  try {
    const result = repairCodexHooks(process.argv[2]);
    console.log(result.changed ? `Removed obsolete ORGII Codex hook from ${result.hooksPath}` : "No obsolete ORGII Codex hook found.");
  } catch (error) {
    console.error(`Could not repair Codex hooks: ${error.message}`);
    process.exitCode = 1;
  }
}

module.exports = { removeLegacyOrgiiCodexHooks, repairCodexHooks };
