//! Observable history, reads and reminder boundaries exercise the shared sink without its containers.
use super::*;

/// Stable IDs and one shared run survive window changes, while ordinary output never raises an alert.
#[test]
fn cloned_sources_retain_attribution_and_ignore_ordinary_records() {
    let logs = RuntimeLogs::default();
    let background = logs.clone();
    let first = logs.append("first", LogLevel::Info, "wasi/stdout", "ready");
    let second = background.append("second", LogLevel::Warning, "lsp/analysis", "slow startup");
    assert!(second > first);
    assert_eq!(logs.generation(), 2);
    assert_eq!(logs.records("first")[0].source, "wasi/stdout");
    assert_eq!(logs.records("first")[0].message, "ready");
    assert!(logs.unread_severity("first").is_none());
    assert_eq!(
        logs.pending_reminders(),
        vec![("second".into(), LogLevel::Warning)]
    );
    assert!(RuntimeLogs::default().records("first").is_empty());
}

/// Latest summary and highest unread severity are independent when an error precedes a warning.
#[test]
fn summary_reads_leave_older_errors_unread_but_confirm_only_captured_reminders() {
    let logs = RuntimeLogs::default();
    let error = logs.append("plugin", LogLevel::Error, "wasm/event", "old error");
    let warning = logs.append("plugin", LogLevel::Warning, "wasi/stderr", "new warning");
    let summary = logs.latest_anomalies();
    let checkpoint = logs.reminder_checkpoint();
    let concurrent = logs.append("peer", LogLevel::Error, "lsp/peer", "late peer error");
    assert_eq!(summary[0].id, warning);
    assert_eq!(logs.unread_severity("plugin"), Some(LogLevel::Error));
    assert!(logs.mark_read("plugin", &[warning]));
    assert!(logs.confirm_reminders(&checkpoint));
    assert_eq!(logs.unread_severity("plugin"), Some(LogLevel::Error));
    assert_eq!(
        logs.pending_reminders(),
        vec![("peer".into(), LogLevel::Error)]
    );
    assert!(!logs.mark_read("plugin", &[concurrent]));
    assert!(logs.view_through("plugin", warning));
    assert!(logs.unread_severity("plugin").is_none());
    assert_eq!(logs.records("plugin")[0].id, error);
    assert_eq!(logs.records("plugin")[0].level, LogLevel::Error);
}

/// A new same-plugin failure after a UI snapshot remains unread and can trigger the next reminder.
#[test]
fn concurrent_arrivals_are_not_cleared_by_old_full_page_or_summary_boundaries() {
    let logs = RuntimeLogs::default();
    let old = logs.append("plugin", LogLevel::Warning, "host", "old warning");
    let checkpoint = logs.reminder_checkpoint();
    let new = logs.append("plugin", LogLevel::Error, "host", "new error");
    assert!(logs.confirm_reminders(&checkpoint));
    assert!(logs.view_through("plugin", old));
    assert_eq!(logs.unread_severity("plugin"), Some(LogLevel::Error));
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Error)]
    );
    assert!(!logs.view_through("plugin", old));
    assert!(logs.mark_read("plugin", &[new]));
    assert!(logs.pending_reminders().is_empty());
}

/// History reads expire at eviction while an exact summary read can release the bounded reminder cause.
/// Unicode truncation never splits a scalar and another plugin's history remains independent.
#[test]
fn retention_is_bounded_per_plugin_and_message_at_unicode_boundaries() {
    let logs = RuntimeLogs::default();
    let peer = logs.append("peer", LogLevel::Error, "host", "peer error");
    let expired = logs.append("noisy", LogLevel::Error, "host", "expired error");
    for index in 0..RECORDS_PER_PLUGIN {
        logs.append(
            "noisy",
            LogLevel::Info,
            "wasi/stdout",
            format!("line {index}"),
        );
    }
    assert_eq!(logs.records("noisy").len(), RECORDS_PER_PLUGIN);
    assert_eq!(logs.records("noisy")[0].message, "line 0");
    assert!(logs.unread_severity("noisy").is_none());
    assert_eq!(
        logs.pending_reminders(),
        vec![
            ("noisy".into(), LogLevel::Error),
            ("peer".into(), LogLevel::Error)
        ]
    );
    assert!(logs.mark_read("noisy", &[expired]));
    assert!(!logs.mark_read("noisy", &[expired]));
    assert_eq!(
        logs.pending_reminders(),
        vec![("peer".into(), LogLevel::Error)]
    );
    assert_eq!(logs.records("peer")[0].id, peer);
    logs.append(
        "noisy",
        LogLevel::Info,
        "wasi/stdout",
        "中".repeat(MESSAGE_CHAR_LIMIT + 1),
    );
    assert_eq!(
        logs.records("noisy").last().unwrap().message,
        "中".repeat(MESSAGE_CHAR_LIMIT)
    );
}

/// IDs are serialized across producers; an impossible future read request cannot pre-read new records.
#[test]
fn parallel_sources_have_unique_ids_and_future_checkpoints_do_not_hide_arrivals() {
    let logs = RuntimeLogs::default();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            let logs = &logs;
            scope.spawn(move || {
                for _ in 0..64 {
                    logs.append("plugin", LogLevel::Info, "background", "message");
                }
            });
        }
    });
    let records = logs.records("plugin");
    assert_eq!(records.len(), 256);
    assert!(records.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert!(logs.view_through("plugin", u64::MAX));
    assert!(!logs.confirm_reminders(&[("plugin".into(), u64::MAX)]));
    logs.append("plugin", LogLevel::Error, "host", "later failure");
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Error)]
    );
}

/// Ordinary output fills the public history budget without acknowledging an earlier anomaly.
fn flood_ordinary_output(logs: &RuntimeLogs, plugin: &str) {
    for index in 0..RECORDS_PER_PLUGIN {
        logs.append(
            plugin,
            LogLevel::Info,
            "plugin.stdout",
            format!("line {index}"),
        );
    }
}

/// A bottom reminder must survive ordinary traffic even when the complete-log tab has no retained anomaly.
#[test]
fn evicted_error_remains_pending_until_the_captured_popup_round_is_confirmed() {
    let logs = RuntimeLogs::default();
    let error = logs.append(
        "plugin",
        LogLevel::Error,
        "host.operation",
        "startup failed",
    );
    flood_ordinary_output(&logs, "plugin");
    let records = logs.records("plugin");
    assert_eq!(records.len(), RECORDS_PER_PLUGIN);
    assert!(records.iter().all(|record| record.level == LogLevel::Info));
    assert_eq!(logs.unread_severity("plugin"), None);
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Error)]
    );
    let (checkpoints, summaries) = logs.reminder_snapshot();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].id, error);
    assert_eq!(summaries[0].plugin, "plugin");
    assert_eq!(summaries[0].source, "host.operation");
    assert_eq!(summaries[0].message, "startup failed");
    assert_eq!(
        checkpoints,
        vec![("plugin".into(), records.last().unwrap().id)]
    );
    assert!(logs.confirm_reminders(&checkpoints));
    assert!(logs.pending_reminders().is_empty());
    assert!(logs.latest_anomalies().is_empty());
    assert!(!logs.mark_read("plugin", &[error]));
    assert_eq!(logs.records("plugin"), records);
}

/// A newer warning summary does not downgrade an unviewed error retained only as a reminder cause.
#[test]
fn evicted_error_takes_precedence_over_the_latest_retained_warning() {
    let logs = RuntimeLogs::default();
    let error = logs.append("plugin", LogLevel::Error, "host.operation", "old failure");
    flood_ordinary_output(&logs, "plugin");
    let warning = logs.append("plugin", LogLevel::Warning, "plugin.stderr", "new warning");
    let (checkpoints, summaries) = logs.reminder_snapshot();
    assert_eq!(summaries[0].id, warning);
    assert_eq!(logs.latest_anomalies(), summaries);
    assert_eq!(logs.unread_severity("plugin"), Some(LogLevel::Warning));
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Error)]
    );
    assert!(logs.mark_read("plugin", &[warning]));
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Error)]
    );
    assert!(logs.confirm_reminders(&checkpoints));
    assert!(logs.pending_reminders().is_empty());
    assert!(!logs.mark_read("plugin", &[error]));
    // Retained read history remains reviewable; the extra evicted cause expires on confirmation.
    assert_eq!(logs.latest_anomalies()[0].id, warning);
}

/// A popup owns record clones, so later same-level representative replacement cannot rewrite its cause.
#[test]
fn captured_snapshot_survives_replacement_without_confirming_the_new_representative() {
    let logs = RuntimeLogs::default();
    let old = logs.append(
        "plugin",
        LogLevel::Error,
        "host.operation",
        "captured failure",
    );
    flood_ordinary_output(&logs, "plugin");
    let (checkpoints, summaries) = logs.reminder_snapshot();
    let through = checkpoints[0].1;
    let new = logs.append("plugin", LogLevel::Error, "host.operation", "later failure");
    flood_ordinary_output(&logs, "plugin");
    logs.append("peer", LogLevel::Warning, "plugin.stderr", "peer warning");
    assert_eq!(summaries[0].id, old);
    assert_eq!(summaries[0].message, "captured failure");
    assert_eq!(
        logs.latest_anomalies()
            .iter()
            .find(|record| record.plugin == "plugin")
            .unwrap()
            .id,
        new
    );
    assert!(logs.confirm_reminders(&checkpoints));
    assert!(!logs.confirm_reminders(&checkpoints));
    assert!(!logs.view_through("plugin", through));
    assert!(!logs.mark_read("plugin", &[old]));
    assert_eq!(logs.unread_severity("plugin"), None);
    assert_eq!(
        logs.pending_reminders(),
        vec![
            ("peer".into(), LogLevel::Warning),
            ("plugin".into(), LogLevel::Error)
        ]
    );
    assert_eq!(summaries[0].id, old);
    assert_eq!(summaries[0].message, "captured failure");
    let (new_checkpoints, new_summaries) = logs.reminder_snapshot();
    assert_eq!(
        new_summaries
            .iter()
            .find(|record| record.plugin == "plugin")
            .unwrap()
            .id,
        new
    );
    assert!(logs.confirm_reminders(&new_checkpoints));
    assert!(logs.pending_reminders().is_empty());
    assert!(!logs.mark_read("plugin", &[new]));
}

/// Viewing a full-page boundary clears its evicted causes, preserving later and other-plugin reminders.
#[test]
fn full_page_view_releases_only_representatives_within_its_captured_boundary() {
    let logs = RuntimeLogs::default();
    let old = logs.append("plugin", LogLevel::Error, "host.operation", "old failure");
    flood_ordinary_output(&logs, "plugin");
    let through = logs.reminder_checkpoint()[0].1;
    let new = logs.append(
        "plugin",
        LogLevel::Warning,
        "plugin.stderr",
        "later warning",
    );
    flood_ordinary_output(&logs, "plugin");
    logs.append("peer", LogLevel::Error, "host.operation", "peer failure");
    assert!(logs.view_through("plugin", through));
    assert!(!logs.view_through("plugin", through));
    assert!(!logs.mark_read("plugin", &[old]));
    assert_eq!(logs.unread_severity("plugin"), None);
    assert_eq!(
        logs.pending_reminders(),
        vec![
            ("peer".into(), LogLevel::Error),
            ("plugin".into(), LogLevel::Warning)
        ]
    );
    assert!(logs.view_through("plugin", new));
    assert!(!logs.mark_read("plugin", &[new]));
    assert_eq!(
        logs.pending_reminders(),
        vec![("peer".into(), LogLevel::Error)]
    );
    assert!(
        logs.latest_anomalies()
            .iter()
            .all(|record| record.plugin == "peer")
    );
    assert_eq!(logs.records("plugin").len(), RECORDS_PER_PLUGIN);
}

/// Per-severity coalescing is bounded; exact-ID reads cannot release a newer replacement by mistake.
#[test]
fn repeated_evictions_keep_only_the_latest_unacknowledged_cause_per_severity() {
    let logs = RuntimeLogs::default();
    let mut previous = Vec::new();
    let mut latest_warning = 0;
    let mut latest_error = 0;
    for _ in 0..3 {
        latest_warning = logs.append("plugin", LogLevel::Warning, "plugin.stderr", "warning");
        latest_error = logs.append("plugin", LogLevel::Error, "host.operation", "failure");
        previous.push((latest_warning, latest_error));
        flood_ordinary_output(&logs, "plugin");
    }
    assert_eq!(logs.records("plugin").len(), RECORDS_PER_PLUGIN);
    assert_eq!(logs.unread_severity("plugin"), None);
    for (warning, error) in previous.into_iter().take(2) {
        assert!(!logs.mark_read("plugin", &[warning, error]));
    }
    assert_eq!(logs.latest_anomalies()[0].id, latest_error);
    assert!(logs.mark_read("plugin", &[latest_error]));
    assert_eq!(
        logs.pending_reminders(),
        vec![("plugin".into(), LogLevel::Warning)]
    );
    assert_eq!(logs.latest_anomalies()[0].id, latest_warning);
    assert!(logs.mark_read("plugin", &[latest_warning]));
    assert!(logs.pending_reminders().is_empty());
    assert!(logs.latest_anomalies().is_empty());
}

/// History already read or covered by a viewed reminder round must not become a new cause when evicted.
#[test]
fn eviction_does_not_preserve_read_or_confirmed_anomalies() {
    let logs = RuntimeLogs::default();
    let read = logs.append("read", LogLevel::Error, "host.operation", "viewed failure");
    assert!(logs.mark_read("read", &[read]));
    let confirmed = logs.append(
        "confirmed",
        LogLevel::Warning,
        "plugin.stderr",
        "confirmed warning",
    );
    assert!(logs.confirm_reminders(&[("confirmed".into(), confirmed)]));
    for plugin in ["read", "confirmed"] {
        flood_ordinary_output(&logs, plugin);
        assert_eq!(logs.unread_severity(plugin), None);
        assert_eq!(logs.records(plugin).len(), RECORDS_PER_PLUGIN);
    }
    assert!(logs.pending_reminders().is_empty());
    assert!(logs.reminder_snapshot().1.is_empty());
    assert!(!logs.mark_read("read", &[read]));
    assert!(!logs.mark_read("confirmed", &[confirmed]));
}
