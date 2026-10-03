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

/// Evicting a record also evicts its read/severity state; Unicode truncation never splits a scalar.
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
    assert!(!logs.mark_read("noisy", &[expired]));
    assert!(logs.unread_severity("noisy").is_none());
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
