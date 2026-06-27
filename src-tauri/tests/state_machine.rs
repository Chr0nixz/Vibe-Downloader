use tauri_app_lib::models::TaskStatus;

#[test]
fn test_legal_transitions() {
    use TaskStatus::*;
    // Queued → various
    assert!(Queued.can_transition_to(Downloading));
    assert!(Queued.can_transition_to(Paused));
    assert!(Queued.can_transition_to(Failed));
    assert!(Queued.can_transition_to(NeedsAttention));
    assert!(Queued.can_transition_to(WaitingNetwork));

    // Downloading → various
    assert!(Downloading.can_transition_to(Paused));
    assert!(Downloading.can_transition_to(Completed));
    assert!(Downloading.can_transition_to(Failed));
    assert!(Downloading.can_transition_to(Retrying));
    assert!(Downloading.can_transition_to(NeedsAttention));
    assert!(Downloading.can_transition_to(WaitingNetwork));

    // Paused → various
    assert!(Paused.can_transition_to(Queued));
    assert!(Paused.can_transition_to(Downloading));
    assert!(Paused.can_transition_to(Failed));

    // Retrying → various
    assert!(Retrying.can_transition_to(Downloading));
    assert!(Retrying.can_transition_to(Paused));
    assert!(Retrying.can_transition_to(Failed));
    assert!(Retrying.can_transition_to(NeedsAttention));

    // WaitingNetwork → various
    assert!(WaitingNetwork.can_transition_to(Queued));
    assert!(WaitingNetwork.can_transition_to(Downloading));
    assert!(WaitingNetwork.can_transition_to(Failed));

    // NeedsAttention → various
    assert!(NeedsAttention.can_transition_to(Queued));
    assert!(NeedsAttention.can_transition_to(Failed));

    // Failed → various
    assert!(Failed.can_transition_to(Queued));
    assert!(Failed.can_transition_to(Retrying));
    assert!(Failed.can_transition_to(NeedsAttention));
}

#[test]
fn test_illegal_transitions() {
    use TaskStatus::*;
    // Completed is a terminal state — no transitions out
    assert!(!Completed.can_transition_to(Downloading));
    assert!(!Completed.can_transition_to(Queued));
    assert!(!Completed.can_transition_to(Failed));
    assert!(!Completed.can_transition_to(Paused));

    // Can't jump to Completed from non-downloading states
    assert!(!Queued.can_transition_to(Completed));
    assert!(!Paused.can_transition_to(Completed));
    assert!(!Failed.can_transition_to(Completed));
    assert!(!NeedsAttention.can_transition_to(Completed));

    // Can't go back to Queued from Downloading (must pause first)
    assert!(!Downloading.can_transition_to(Queued));

    // Can't go to Retrying from non-downloading/non-failed states
    assert!(!Queued.can_transition_to(Retrying));
    assert!(!Paused.can_transition_to(Retrying));

    // Same-status transitions are not "transitions" (progress updates bypass this)
    assert!(!Queued.can_transition_to(Queued));
    assert!(!Downloading.can_transition_to(Downloading));
    assert!(!Completed.can_transition_to(Completed));
}

#[test]
fn test_completed_is_terminal() {
    use TaskStatus::*;
    for target in [
        Queued,
        Downloading,
        Paused,
        Failed,
        Retrying,
        WaitingNetwork,
        NeedsAttention,
    ] {
        assert!(
            !Completed.can_transition_to(target),
            "Completed should not transition to {:?}",
            target
        );
    }
}
