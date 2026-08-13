//! Canonical Journey graph construction from orgtrack canonical records.
//!
//! This is the real data path behind `journey_graph_query`: it reads the
//! persisted canonical stores (sessions / edit artifacts / commit links),
//! filters them to the requested scope, runs the canonical projector
//! (`orgtrack_graph::journey::project_canonical_journey`) and fails closed
//! when the independent audit reports uncovered canonical units.
//!
//! The persisted canonical graph is not optional: absent scope data is an
//! error, never a synthesized partial response.

use std::collections::{HashMap, HashSet};

use database::db::get_projects_connection;
use core_types::session::ParentSessionRelation;
use orgtrack_core::canonical::{SessionEditKind, SessionRecord};
use orgtrack_core::store::sqlite::SqliteRecordStore;
use orgtrack_core::store::RecordStore;
use orgtrack_graph::audit::audit_canonical_journey;
use orgtrack_graph::journey::{
    ArtifactRelation, CanonicalArtifact, CanonicalCommit, CanonicalJourneyInput, CanonicalProject,
    CanonicalSession, CanonicalTurn, CoverageStatus, JourneyGraph,
};
use orgtrack_graph::JourneyScope;
use rusqlite::OptionalExtension;
use session_persistence::{load_events, load_turn_index, CachedTurnSummary};

/// Build the canonical Journey graph for a scope.
///
/// Scope semantics (no guessing, no synthesized data):
/// - `project/{id}` resolves the canonical project id through its linked
///   workspaces (`linked_repos_json` in projects.db) and selects every
///   canonical session whose `workspace_path` is inside one of them. A
///   project without explicit linked workspaces is an error.
/// - `session/{id}` follows only explicitly compact-continuation parents.
pub fn build_journey_graph(
    store: &SqliteRecordStore,
    scope: &JourneyScope,
) -> Result<JourneyGraph, String> {
    let sessions = store.list_sessions(None)?;
    let artifacts = store.list_edit_artifacts(None, None)?;
    let commits = store.list_commit_links()?;

    let selected = select_sessions(&sessions, scope)?;
    if selected.is_empty() {
        return Err(format!(
            "canonical Journey graph store is not initialized for this project; \
             no canonical sessions for scope {}",
            scope_label(scope)
        ));
    }

    let mut input = CanonicalJourneyInput::default();
    let mut seen_projects: HashSet<String> = HashSet::new();
    for session in &sessions {
        if !selected.contains(&session.session_id) {
            continue;
        }
        // A project-scoped graph keeps the selected canonical project id as
        // its sole project node. Session scopes have no selected project id,
        // so their honest project grouping remains the recorded workspace.
        let project_id = match scope {
            JourneyScope::Project(id) => id.clone(),
            JourneyScope::Session(_) => session
                .workspace_path
                .clone()
                .unwrap_or_else(|| "unassigned".to_string()),
        };
        if seen_projects.insert(project_id.clone()) {
            input.projects.push(CanonicalProject {
                id: project_id.clone(),
                source_ref: match scope {
                    JourneyScope::Project(id) => format!("project-store:{id}"),
                    JourneyScope::Session(_) => format!("orgtrack:project:{project_id}"),
                },
            });
        }
        // Only the canonical child record's explicit compact relation is
        // lineage evidence. A raw parent ID, a delegated parent, or an
        // unknown relation remains ordinary metadata with no Journey edge.
        let compacted_to = compact_parent_id(session)
            .filter(|parent| selected.contains(parent.as_str()));
        input.sessions.push(CanonicalSession {
            id: session.session_id.clone(),
            project_id,
            work_item_id: None,
            resumed_from: None,
            compacted_to,
            forked_from: None,
            source_ref: format!("orgtrack:session:{}", session.session_id),
            display_timestamp: session.created_at.clone(),
            title: non_empty(session.title.clone()),
            lifecycle_status: session.status.clone(),
            branch: session.branch.clone(),
        });
    }

    // The session turn index is the canonical local transcript projection: it
    // preserves the user-side prompt/summary, explicit status, interruption,
    // and display time for every materialized turn. Do not substitute edit
    // artifacts here: an edit is evidence of a file operation, not evidence
    // of what the turn was trying to accomplish.
    for session_id in &selected {
        let assistant_results = load_assistant_turn_results(session_id)?;
        for turn in load_turn_index(session_id).map_err(|err| {
            format!("cannot load canonical turn index for session {session_id}: {err}")
        })? {
            input.turns.push(CanonicalTurn {
                session_id: session_id.clone(),
                sequence: turn.start_sequence.max(0) as u64,
                source_ref: format!("session_turn:{}:{}", session_id, turn.turn_id),
                display_timestamp: Some(turn.started_at.clone()),
                summary: non_empty(turn.user_preview.clone()),
                result_summary: terminal_turn_result(&assistant_results, &turn),
                lifecycle_status: Some(turn_lifecycle_status(&turn.status, turn.interrupted)),
            });
        }
    }

    for artifact in &artifacts {
        if !selected.contains(&artifact.session_id) {
            continue;
        }
        let relation = match artifact.edit_kind {
            SessionEditKind::Write => ArtifactRelation::Produced,
            SessionEditKind::Patch | SessionEditKind::Delete => ArtifactRelation::Modified,
            // Read / CommitBoundary / Unknown carry no produced/modified
            // evidence edge; they must not appear as canonical file lineage.
            _ => continue,
        };
        input.artifacts.push(CanonicalArtifact {
            source: artifact.source.clone(),
            id: artifact.record_id.clone(),
            session_id: artifact.session_id.clone(),
            file_repo: artifact.workspace_path.clone(),
            file_path: Some(artifact.file_path.clone()),
            relation,
            source_ref: format!("orgtrack:artifact:{}", artifact.record_id),
        });
    }

    for commit in &commits {
        let linked_session = commit
            .session_ids
            .iter()
            .find(|session_id| selected.contains(*session_id));
        if let Some(session_id) = linked_session {
            input.commits.push(CanonicalCommit {
                // CommitLinkRecord carries no repo field; keep the honest
                // placeholder rather than deriving a repo from file paths.
                repo: "unknown".to_string(),
                sha: commit.commit_sha.clone(),
                session_id: Some(session_id.clone()),
                work_item_id: None,
                source_ref: format!("orgtrack:commit:{}", commit.commit_sha),
            });
        }
    }

    let graph = orgtrack_graph::journey::project_canonical_journey(&input)?;
    let report = audit_canonical_journey(&input, &graph);
    if !report.trustworthy {
        let uncovered: Vec<_> = report
            .coverage
            .iter()
            .filter(|entry| matches!(entry.status, CoverageStatus::Uncovered))
            .map(|entry| entry.source_ref.clone())
            .collect();
        return Err(format!(
            "canonical Journey graph store is incomplete for this project; \
             refusing partial data, uncovered canonical units: {}",
            uncovered.join(", ")
        ));
    }
    Ok(graph)
}

fn compact_parent_id(session: &SessionRecord) -> Option<String> {
    (session.metadata.parent_session_relation
        == Some(ParentSessionRelation::CompactContinuation))
        .then(|| session.parent_session_id.clone())
        .flatten()
}

/// A persisted assistant message eligible to be projected as a materialized
/// turn's terminal result. The history sequence is a canonical event boundary,
/// not a presentation-time approximation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AssistantTurnResult {
    history_sequence: i64,
    execution_turn_id: Option<String>,
    text: String,
}

/// Load terminal assistant messages already persisted for a session. A result
/// is only attachable when the turn carries a durable `executionTurnId` that
/// exactly equals the assistant event's `args.turnId`. The materialized
/// `session_turns` start/end history-sequence interval is a leak guard for
/// that exact join: an assistant event with the right execution id but
/// outside the turn's canonical sequence range never counts.
///
/// Legacy turns without a persisted execution id deliberately produce no
/// result — no interval, timestamp, text, or similarity fallback.
fn load_assistant_turn_results(session_id: &str) -> Result<Vec<AssistantTurnResult>, String> {
    let events = load_events(session_id)
        .map_err(|err| format!("cannot load canonical events for session {session_id}: {err}"))?;
    let mut results = Vec::new();
    for event in events {
        if event.function_name.as_deref() != Some("assistant") {
            continue;
        }
        let Some(history_sequence) = event.history_sequence else {
            // A sequence-less legacy event cannot be assigned to a materialized
            // turn range, and without an execution id must stay unprojected.
            continue;
        };
        let execution_turn_id = serde_json::from_str::<serde_json::Value>(&event.args_json)
            .ok()
            .and_then(|args| {
                args.get("turnId")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            });
        let text = serde_json::from_str::<serde_json::Value>(&event.result_json)
            .ok()
            .and_then(|result_json| {
                result_json
                    .get("observation")
                    .or_else(|| result_json.get("content"))
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
            })
            .or_else(|| non_empty(event.content));
        if let Some(text) = text.and_then(non_empty) {
            results.push(AssistantTurnResult {
                history_sequence,
                execution_turn_id,
                text,
            });
        }
    }
    Ok(results)
}

/// Return the final persisted assistant message belonging to this materialized
/// user turn. This projection only accepts the explicit durable DialogTurn
/// identity: `user_message.result_json.executionTurnId` must exactly equal the
/// assistant event's `args.turnId`. Legacy rows without that source identity
/// deliberately produce no result; neither event order, timestamps, nor text
/// may be used as a substitute.
fn terminal_turn_result(
    assistant_results: &[AssistantTurnResult],
    turn: &CachedTurnSummary,
) -> Option<String> {
    let execution_turn_id = turn.execution_turn_id.as_deref()?;
    assistant_results
        .iter()
        .filter(|result| {
            result.execution_turn_id.as_deref() == Some(execution_turn_id)
                && result.history_sequence > turn.start_sequence
                && turn
                    .end_sequence
                    .map(|end| result.history_sequence < end)
                    .unwrap_or(true)
        })
        .max_by_key(|result| result.history_sequence)
        .map(|result| result.text.clone())
}

fn non_empty(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn turn_lifecycle_status(status: &str, interrupted: bool) -> String {
    if interrupted {
        "interrupted".to_string()
    } else {
        status.to_string()
    }
}

fn select_sessions(
    sessions: &[orgtrack_core::canonical::SessionRecord],
    scope: &JourneyScope,
) -> Result<HashSet<String>, String> {
    let mut selected = HashSet::new();
    match scope {
        JourneyScope::Project(id) => {
            // Resolve the canonical project identity to its linked
            // workspaces (`linked_repos_json` in projects.db). The mapping
            // must exist explicitly: a project without linked workspaces is
            // an error, never a guessed path.
            let workspaces = resolve_project_workspaces(id)?;
            for session in sessions {
                let in_project = session
                    .workspace_path
                    .as_deref()
                    .map(|path| {
                        workspaces.iter().any(|workspace| {
                            path == workspace || path.starts_with(&format!("{workspace}/"))
                        })
                    })
                    .unwrap_or(false);
                if in_project {
                    selected.insert(session.session_id.clone());
                }
            }
        }
        JourneyScope::Session(id) => {
            let by_id: HashMap<&str, &orgtrack_core::canonical::SessionRecord> = sessions
                .iter()
                .map(|session| (session.session_id.as_str(), session))
                .collect();
            if !by_id.contains_key(id.as_str()) {
                return Err(format!(
                    "canonical Journey graph store is not initialized for this project; \
                     no canonical session for scope {}",
                    scope_label(scope)
                ));
            }
            let mut cursor = Some(id.clone());
            while let Some(session_id) = cursor {
                if !selected.insert(session_id.clone()) {
                    break;
                }
                cursor = by_id
                    .get(session_id.as_str())
                    .and_then(|session| compact_parent_id(session));
            }
        }
    }
    Ok(selected)
}

/// Read the canonical project row from `projects.db` and return its linked
/// workspace paths (`linked_repos_json`). Fail-closed: an unknown project id
/// or an empty linked-repos list is an error, because there is no honest
/// workspace to scope the journey to.
fn resolve_project_workspaces(project_id: &str) -> Result<Vec<String>, String> {
    let conn = get_projects_connection()
        .map_err(|err| format!("cannot open project store while resolving {project_id}: {err}"))?;
    let linked_repos_json: Option<String> = conn
        .query_row(
            "SELECT linked_repos_json FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|err| format!("cannot read linked repos for {project_id}: {err}"))?;
    let Some(raw) = linked_repos_json else {
        return Err(format!(
            "canonical Journey graph store is not initialized for this project;              project {project_id} does not exist in the project store"
        ));
    };
    let workspaces: Vec<String> = serde_json::from_str(&raw)
        .map_err(|err| format!("project {project_id} linked_repos_json is invalid: {err}"))?;
    if workspaces.is_empty() {
        return Err(format!(
            "canonical Journey graph store is not initialized for this project;              project {project_id} has no linked workspaces (linked_repos_json is empty)"
        ));
    }
    Ok(workspaces)
}

fn scope_label(scope: &JourneyScope) -> String {
    match scope {
        JourneyScope::Project(id) => format!("project/{id}"),
        JourneyScope::Session(id) => format!("session/{id}"),
    }
}

#[cfg(test)]
mod lineage_tests {
    use super::*;
    use orgtrack_core::canonical::AgentMetadata;
    use orgtrack_graph::journey::JourneyEdgeKind;

    fn session(parent_session_id: Option<&str>, relation: Option<ParentSessionRelation>) -> SessionRecord {
        session_with_id("child", parent_session_id, relation)
    }

    fn session_with_id(
        id: &str,
        parent_session_id: Option<&str>,
        relation: Option<ParentSessionRelation>,
    ) -> SessionRecord {
        SessionRecord {
            schema_version: 1,
            source: "test".to_string(),
            source_session_id: id.to_string(),
            session_id: id.to_string(),
            title: id.to_string(),
            status: None,
            created_at: None,
            updated_at: None,
            completed_at: None,
            workspace_path: None,
            branch: None,
            parent_session_id: parent_session_id.map(str::to_string),
            org_member_id: None,
            metadata: AgentMetadata {
                parent_session_relation: relation,
                ..AgentMetadata::default()
            },
        }
    }

    #[test]
    fn session_scope_follows_only_compact_continuation_parents() {
        let sessions = vec![
            session_with_id("parent", None, None),
            session_with_id(
                "child",
                Some("parent"),
                Some(ParentSessionRelation::CompactContinuation),
            ),
            session_with_id(
                "grandchild",
                Some("child"),
                Some(ParentSessionRelation::CompactContinuation),
            ),
            session_with_id(
                "delegated",
                Some("parent"),
                Some(ParentSessionRelation::Delegated),
            ),
            session_with_id("raw", Some("parent"), None),
            session_with_id("root", None, None),
        ];

        // Upward closure follows compact continuations only.
        let selected =
            select_sessions(&sessions, &JourneyScope::Session("child".to_string())).unwrap();
        assert_eq!(
            selected,
            HashSet::from(["child".to_string(), "parent".to_string()])
        );

        let selected =
            select_sessions(&sessions, &JourneyScope::Session("grandchild".to_string())).unwrap();
        assert_eq!(
            selected,
            HashSet::from([
                "grandchild".to_string(),
                "child".to_string(),
                "parent".to_string()
            ])
        );

        // Delegated and raw parents are metadata, never lineage.
        let selected =
            select_sessions(&sessions, &JourneyScope::Session("delegated".to_string())).unwrap();
        assert_eq!(selected, HashSet::from(["delegated".to_string()]));
        let selected =
            select_sessions(&sessions, &JourneyScope::Session("raw".to_string())).unwrap();
        assert_eq!(selected, HashSet::from(["raw".to_string()]));
        let selected =
            select_sessions(&sessions, &JourneyScope::Session("root".to_string())).unwrap();
        assert_eq!(selected, HashSet::from(["root".to_string()]));

        // Unknown scope fails closed instead of guessing.
        assert!(select_sessions(&sessions, &JourneyScope::Session("missing".to_string())).is_err());
    }

    #[test]
    fn session_scope_closure_terminates_on_parent_cycles() {
        let sessions = vec![
            session_with_id(
                "a",
                Some("b"),
                Some(ParentSessionRelation::CompactContinuation),
            ),
            session_with_id(
                "b",
                Some("a"),
                Some(ParentSessionRelation::CompactContinuation),
            ),
        ];
        let selected =
            select_sessions(&sessions, &JourneyScope::Session("a".to_string())).unwrap();
        assert_eq!(
            selected,
            HashSet::from(["a".to_string(), "b".to_string()])
        );
    }

    #[test]
    fn raw_and_delegated_parents_do_not_become_journey_lineage() {
        assert_eq!(compact_parent_id(&session(Some("parent"), None)), None);
        assert_eq!(
            compact_parent_id(&session(Some("parent"), Some(ParentSessionRelation::Delegated))),
            None
        );
    }

    #[test]
    fn compact_continuation_projects_a_compacted_to_edge() {
        let compact_child = session(
            Some("parent"),
            Some(ParentSessionRelation::CompactContinuation),
        );
        let input = CanonicalJourneyInput {
            projects: vec![CanonicalProject { id: "p".to_string(), source_ref: "test".to_string() }],
            work_items: vec![],
            sessions: vec![
                CanonicalSession {
                    id: "parent".to_string(), project_id: "p".to_string(), work_item_id: None,
                    resumed_from: None, compacted_to: None, forked_from: None, source_ref: "parent".to_string(),
                    display_timestamp: None, title: None, lifecycle_status: None, branch: None,
                },
                CanonicalSession {
                    id: compact_child.session_id.clone(), project_id: "p".to_string(), work_item_id: None,
                    resumed_from: None, compacted_to: compact_parent_id(&compact_child), forked_from: None,
                    source_ref: "child".to_string(), display_timestamp: None, title: None,
                    lifecycle_status: None, branch: None,
                },
            ],
            turns: vec![], artifacts: vec![], commits: vec![],
        };
        let graph = orgtrack_graph::journey::project_canonical_journey(&input).unwrap();
        assert!(graph.edges.iter().any(|edge| {
            edge.kind == JourneyEdgeKind::CompactedTo
                && edge.from == "session/child"
                && edge.to == "session/parent"
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(
        user_event_id: &str,
        execution_turn_id: Option<&str>,
        start_sequence: i64,
        end_sequence: Option<i64>,
    ) -> CachedTurnSummary {
        CachedTurnSummary {
            session_id: "session-1".to_string(),
            turn_id: user_event_id.to_string(),
            turn_intent_id: Some("submitted-intent-8".to_string()),
            execution_turn_id: execution_turn_id.map(str::to_owned),
            start_sequence,
            end_sequence,
            next_turn_id: None,
            started_at: "2026-08-03T00:00:00Z".to_string(),
            ended_at: None,
            duration_ms: None,
            user_event_ids: vec![user_event_id.to_string()],
            user_preview: "inspect Journey".to_string(),
            event_count: 1,
            body_event_count: 0,
            status: "completed".to_string(),
            interrupted: false,
            modified_files: Vec::new(),
        }
    }

    fn result(sequence: i64, execution_turn_id: Option<&str>, text: &str) -> AssistantTurnResult {
        AssistantTurnResult {
            history_sequence: sequence,
            execution_turn_id: execution_turn_id.map(str::to_owned),
            text: text.to_string(),
        }
    }

    #[test]
    fn terminal_result_join_uses_execution_turn_id_not_user_or_intent_id() {
        let assistant_results = vec![
            result(9, Some("execution-turn-9"), "terminal assistant result"),
            result(99, Some("execution-turn-9"), "must not leak from another turn"),
        ];
        let persisted_user_event_id = "user-message-factual-7";
        let submitted_intent_id = "submitted-intent-8";

        // All three identities are deliberately different. Explicit
        // assistant args.turnId is the DialogTurn identity only.
        assert_ne!(persisted_user_event_id, "execution-turn-9");
        assert_ne!(submitted_intent_id, "execution-turn-9");
        assert_eq!(
            terminal_turn_result(
                &assistant_results,
                &turn(persisted_user_event_id, Some("execution-turn-9"), 2, Some(10)),
            ),
            Some("terminal assistant result".to_string())
        );
        assert_eq!(
            terminal_turn_result(
                &assistant_results,
                &turn(persisted_user_event_id, Some(persisted_user_event_id), 2, Some(10)),
            ),
            None
        );
        assert_eq!(
            terminal_turn_result(
                &assistant_results,
                &turn(persisted_user_event_id, Some(submitted_intent_id), 2, Some(10)),
            ),
            None
        );
    }

    #[test]
    fn legacy_turn_without_persisted_execution_id_has_no_result() {
        let assistant_results = vec![
            result(3, Some("runtime-a"), "nearby assistant text is not enough"),
            result(7, Some("runtime-a"), "terminal-looking text is not enough"),
        ];

        // This historical row has a user-event id and a submitted-intent id,
        // but no durable execution/DialogTurn id. Do not infer the answer
        // from sequence interval, timestamp, content, or a nearby assistant
        // event. It remains intentionally blank until the source contract
        // emits the exact join key for new turns.
        assert_eq!(
            terminal_turn_result(
                &assistant_results,
                &turn("user-message-legacy", None, 2, Some(8)),
            ),
            None
        );
    }

    #[test]
    fn explicit_execution_id_without_matching_assistant_has_no_result() {
        let assistant_results = vec![result(8, Some("next-turn"), "unrelated result")];

        assert_eq!(
            terminal_turn_result(
                &assistant_results,
                &turn("user-message-legacy", Some("execution-a"), 2, Some(8)),
            ),
            None
        );
    }
}
