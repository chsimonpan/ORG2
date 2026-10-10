# Reactive rejection -> next-turn durable fork/checkpoint (job193)

Baseline f1d33369f. Target R3 joined runtime compaction transition from job173-seven-gates-remaining.md; does not close rendered GUI acceptance or any historical 200+49 row.

New opt-in real default Wry test calls actual run_reactive_compaction then actual run_pre_turn_compaction with managed AgentAppState, sandbox DB and in-process provider. Reactive success leaves gateway binding, running parent and weighted spend/generation unchanged; next eligible turn forks, correlates lock/seed/publication, reopens durable binding, archives parent, persists child history, resets parent checkpoint once and starts child spend at zero. Production reactive path explicitly cannot fork a provider-rejected prefix; no reactive fork behavior invented.

job192 red: single oversized next-turn message produced no compactable segment / no fork redirect. job193 fixture corrected to four actual user/assistant pairs, not production bypass. Real Wry test 1 passed; queued lock/cancel capture 1 passed; concurrent durable winner 1 passed. Overall exit0. Separate cargo processes, no broad GTK filter.

Fixed original c51c2f image (not installation snapshot), uid1000, network none, cached offline locked Rust, private MIT auth and Xvfb -nolisten tcp. No installation/new image. Driver PPID1. Cleanup verified no driver/Xvfb/private auth directory remains; DSH untouched.

Cargo.lock SHA256 21b6c616203ca43c5b3f00e34a2e0ea4612601d0b815c61c1ea23aae20cccd45 unchanged, diff --check green. Logs staging dependency-native-logs/job193-{entry,regression,host,xvfb}.log and job193.exit; red job192 retained. Test only; no real model, GUI-human visual claim, historical >60s root-cause claim, ledger acceptance, push or deployment. Existing untracked agent-core/crates/ untouched.

Remaining seven gates unchanged: R1 authorized routed-provider auto5M; R2 desktop permission CRUD/revoke and explicit code_map retirement decision; R3 rendered joined GUI interaction; R4 original incident timeline or acknowledged unknowability plus authorized actual-provider responsiveness/cancel/recovery; R5 provider golden query and rendered owner/scope; R6 model-search visual check; R7 joined frontend/native huge-file desktop RSS/UX acceptance. Further same-fixture stacking is not gate closure.
