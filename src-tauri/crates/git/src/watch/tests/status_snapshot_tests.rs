use crate::watch::debounce::status_snapshot_changed;
use crate::watch::types::GitStatus;

#[test]
fn unchanged_status_snapshot_does_not_publish() {
    let status = GitStatus::default();

    assert!(!status_snapshot_changed(Some(&status), &status));
}

#[test]
fn changed_status_snapshot_publishes() {
    let previous = GitStatus::default();
    let mut current = previous.clone();
    current.ahead = 1;

    assert!(status_snapshot_changed(Some(&previous), &current));
}

#[test]
fn first_status_snapshot_publishes() {
    assert!(status_snapshot_changed(None, &GitStatus::default()));
}
