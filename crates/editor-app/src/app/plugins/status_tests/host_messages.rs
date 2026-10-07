//! Native host acknowledgement and clearing preserve publicly installed plugins' logs and reminders.

use super::*;
use crate::app::messages::MessageLevel;

/// Host-reported plugin errors remain plugin-owned; viewing and clearing host history cannot read them.
#[gpui::test]
fn host_messages_confirmation_and_clear_preserve_plugin_logs(cx: &mut TestAppContext) {
    with_status(cx, 2, "en", false, |form, app, manager| {
        let logs = manager.runtime_logs();
        logs.append(
            "status-00",
            LogLevel::Warning,
            "plugin.stderr",
            "Plugin warning remains unread",
        );
        logs.append(
            "status-01",
            LogLevel::Error,
            "host.operation",
            "Host-reported plugin failure remains unread",
        );
        let records = [logs.records("status-00"), logs.records("status-01")];
        let generation = logs.generation();
        let reminders = logs.pending_reminders();
        assert_eq!(
            reminders,
            vec![
                ("status-00".into(), LogLevel::Warning),
                ("status-01".into(), LogLevel::Error),
            ]
        );
        // RuntimeLogs exposes no individual read flag. Its change counter includes reads and
        // reminder acknowledgements; equality plus one anomaly per owner proves neither is read.
        let assert_plugins_unchanged = |form: &mut VisualTestContext| {
            assert_eq!(logs.records("status-00"), records[0]);
            assert_eq!(logs.records("status-01"), records[1]);
            assert_eq!(logs.generation(), generation);
            assert_eq!(logs.pending_reminders(), reminders);
            assert_eq!(logs.unread_severity("status-00"), Some(LogLevel::Warning));
            assert_eq!(logs.unread_severity("status-01"), Some(LogLevel::Error));
            assert!(form.debug_bounds("plugin-error-indicator").is_some());
            assert!(form.debug_bounds("plugin-status-popup").is_none());
        };
        publish(form, app);
        assert!(form.debug_bounds("host-messages-empty").is_some());
        assert!(form.debug_bounds("host-message-1").is_none());
        assert!(form.debug_bounds("host-messages-dot").is_none());
        assert_plugins_unchanged(form);

        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.record_host_message(MessageLevel::Error, "Host operation failed", cx);
            });
        });
        draw(form);
        assert!(form.debug_bounds("host-message-1").is_some());
        assert!(form.debug_bounds("host-message-error").is_some());
        assert!(form.debug_bounds("host-messages-dot").is_some());
        assert_plugins_unchanged(form);

        // Both transitions use the actual bottom button: hiding preserves the host alert and
        // opening acknowledges only that history, never the neighboring plugin reminder.
        click(form, "host-messages-toggle");
        assert!(form.debug_bounds("host-messages-panel").is_none());
        assert!(form.debug_bounds("host-messages-dot").is_some());
        assert_plugins_unchanged(form);
        click(form, "host-messages-toggle");
        assert!(form.debug_bounds("host-message-1").is_some());
        assert!(form.debug_bounds("host-messages-dot").is_none());
        assert_plugins_unchanged(form);

        click(form, "host-messages-clear");
        assert!(form.debug_bounds("host-messages-empty").is_some());
        assert!(form.debug_bounds("host-message-1").is_none());
        assert!(form.debug_bounds("host-messages-dot").is_none());
        assert_plugins_unchanged(form);

        // A receipt arriving after clear owns a fresh host reminder while plugin checkpoints stay put.
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.record_host_message(MessageLevel::Warning, "New host warning", cx);
            });
        });
        draw(form);
        assert!(form.debug_bounds("host-message-2").is_some());
        assert!(form.debug_bounds("host-message-warning").is_some());
        assert!(form.debug_bounds("host-messages-dot").is_some());
        assert_plugins_unchanged(form);
    });
}
