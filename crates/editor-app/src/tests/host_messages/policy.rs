//! Routine operations stay quiet while attention-worthy host results retain their existing destination.

use super::*;
use crate::app::messages::MessageLevel;

/// An invalid clipboard file uses the transfer path while publishing the same visible host error receipt.
#[gpui::test]
fn host_messages_failed_file_transfer_reaches_history(cx: &mut TestAppContext) {
    with_messages(cx, |form, app, root| {
        form.update(|window, cx| {
            cx.write_to_clipboard(gpui_kit::ClipboardItem {
                entries: vec![gpui_kit::ClipboardEntry::ExternalPaths(
                    gpui_kit::ExternalPaths(vec![root.join("missing-transfer.txt")].into()),
                )],
            });
            app.update(cx, |app, cx| {
                app.paste_explorer_path(&root, true, window, cx);
            });
        });
        draw(form);
        assert!(form.debug_bounds("host-message-error").is_some());
        assert!(form.debug_bounds("host-messages-dot").is_some());
        assert!(form.debug_bounds("local-notification").is_some());
        assert!(!root.join("missing-transfer.txt").exists());
    });
}

/// Exercise source policy through actual document, tree and theme operations, not translated text filtering.
#[gpui::test]
fn host_messages_routine_operations_stay_quiet(cx: &mut TestAppContext) {
    with_messages(cx, |form, app, root| {
        let path = root.join("document.txt");
        std::fs::write(&path, "original").unwrap();
        form.update(|window, cx| app.update(cx, |app, cx| app.open_file(path.clone(), window, cx)));
        draw(form);
        assert!(
            form.debug_bounds("host-messages-empty").is_some(),
            "opening a document is quiet"
        );
        form.simulate_input("changed");
        form.update(|_, cx| app.update(cx, |app, cx| app.save_current(cx)));
        draw(form);
        assert!(std::fs::read_to_string(&path).unwrap().contains("changed"));
        assert!(
            form.debug_bounds("host-messages-empty").is_some(),
            "saving a document is quiet"
        );
        form.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.save_current(cx);
                app.toggle_theme(window, cx);
                app.on_refresh_action(&RefreshWorkspace, window, cx);
                app.start_explorer_edit(ExplorerEditKind::File, root.clone(), true, window, cx);
            })
        });
        draw(form);
        form.simulate_input("created.txt");
        form.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.finish_explorer_edit(cx);
                app.close_tab(path.clone(), window, cx);
            })
        });
        draw(form);
        assert!(root.join("created.txt").exists());
        assert!(
            form.debug_bounds("host-messages-empty").is_some(),
            "routine source operations produce no receipts"
        );
        let group = form.debug_bounds("plugin-windows-group").unwrap();
        let button = form.debug_bounds("host-messages-toggle").unwrap();
        assert!(
            group.contains(&button.center()),
            "the message button belongs to the window tool group"
        );
        assert!(
            button.left() < px(100.),
            "the window group remains at the lower left"
        );
        form.update(|window, cx| {
            app.update(cx, |app, cx| {
                app.open_file(root.join("missing.txt"), window, cx)
            })
        });
        draw(form);
        assert!(form.debug_bounds("host-message-error").is_some());
        assert!(form.debug_bounds("host-messages-dot").is_some());
        form.update(|_, cx| {
            app.update(cx, |app, cx| {
                app.record_host_message(MessageLevel::Info, "Important host result", cx)
            })
        });
        draw(form);
        assert!(
            form.debug_bounds("host-message-info").is_some(),
            "important information remains supported"
        );
        assert!(
            form.debug_bounds("host-messages-dot").is_some(),
            "information never acknowledges an earlier error"
        );
    });
}
