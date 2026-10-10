# Real Wry / in-container Xvfb validation

Base: `sha256:c51c2f9609a0b09d919c82bfcdba53df97c0a7d50432b67f786a6bed59cddfcd` (Ubuntu ancestor label `sha256:5ec03bb3441e8b0bf3b4f9cd4629a1ae763010dc3035bb8da3ae6cf026486401`).
Reusable installation snapshot: `org2-xvfb:184-reusable`, digest `sha256:d21ca6e6dfc4f2302d6897d4ea5efd7fdbc3f50134cfff686c81d854c47d566b`.

Root package-only container, no mounts: apt update + simulated install + `apt-get install --no-upgrade --no-install-recommends xvfb xauth`. Discovery: fixed base already contains Xvfb `2:21.1.4-2ubuntu1.7~22.04.16`, xauth `1:1.1-1build2`; zero new or upgraded packages. Before/after package manifests identical. Do not reinstall in a loop. Evidence in staging `dependency-native-logs/job184-{packages-before,packages-after,apt-plan}`. Initial invocation exit126 was duplicate bash entrypoint, not a package failure; corrected install exit0.

All runtime validation: uid1000, network none, private rust-cache, no host display sockets/auth/credentials/business DB mounts. Xvfb and test executable are in same container. Private MIT cookie, `-nolisten tcp`; EXIT trap terminates/waits Xvfb and removes auth directory. Display probe requires both XOpenDisplay and gtk_init_check success.

- job184: display green, but cargo matched zero tests (not entry success).
- job185: exit101, test-only Tauri context feature absent.
- job186: real default Wry Builder + managed AgentAppState, 1 passed, exit0.
- job187: exit101 because two Wry tests in one harness use different threads and GTK main context is already owned. Run the two real-Wry tests in separate cargo processes; `--test-threads=1` alone is insufficient.
- job188: exit101 preturn did not redirect; fixture threshold/window did not force real compaction.
- job189: exit101 fixture edit compile error; corrected without changing production code.
- job190: real preturn -> fork green, 1 passed exit0.
- job191: final opt-in `real-wry-tests` preturn fixture green (1 passed), processor queued lock/cancel capture regression green (1 passed), concurrent durable fork winner regression green (1 passed), overall exit0.

Final fixture uses sandbox DB + in-process provider, real default Wry runtime, managed state and actual run_pre_turn_compaction -> attempt_fork path. It asserts ForkRedirect, same request across lock/seed/publication capture, reopened durable binding, archived parent, persisted child history, parent checkpoint generation increment/reset and zero child weighted usage. No real model requests.

Commands: `cargo test --locked --offline --manifest-path src-tauri/Cargo.toml -p agent_core --features real-wry-tests --lib compaction_real_wry_preturn_fork_publication -- --test-threads=1 --nocapture`; run `compaction_real_wry_display_entry_probe` separately. Do not use the broad `compaction_real_wry_` filter in one process.

Cargo.lock SHA256 unchanged: `21b6c616203ca43c5b3f00e34a2e0ea4612601d0b815c61c1ea23aae20cccd45`. Existing untracked `src-tauri/crates/agent-core/crates/` untouched/un-staged. Original 60s unknown/accepted2490 unchanged. No push/deploy, no DSH/OpenClaw/online edits.
