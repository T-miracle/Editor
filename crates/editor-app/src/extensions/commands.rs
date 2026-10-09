//! Host features invoke installed plugin commands without depending on their implementation.
use super::*;

/// One immutable worker publication supplies both discoverable operations and command ownership.
pub(crate) struct ShortcutSnapshot {
    pub(crate) trusted: bool,
    pub(crate) ready: bool,
    pub(crate) entries: Vec<Installed>,
    pub(crate) startup: BTreeMap<String, String>,
    pub(crate) commands: BTreeMap<(String, String), u64>,
}

impl ExtensionPanel {
    /// Read metadata and command epochs under one lock so consumers cannot mix incarnations.
    pub(crate) fn shortcut_snapshot(&self) -> ShortcutSnapshot {
        let state = self.worker.state.lock().unwrap();
        let trusted = self
            .worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire);
        let mut commands = BTreeMap::new();
        if trusted {
            for entry in &state.entries {
                for command in &entry.manifest.commands {
                    if let Some(epoch) = state.command_epoch(&entry.manifest.id, &command.id) {
                        commands.insert((entry.manifest.id.clone(), command.id.clone()), epoch);
                    }
                }
            }
        }
        ShortcutSnapshot {
            trusted,
            ready: state.ready,
            entries: state.entries.clone(),
            startup: state.startup.clone(),
            commands,
        }
    }

    /// Capture the latest ready identity; sequence dispatch must retain this epoch until completion.
    pub(crate) fn shortcut_epoch(&self, plugin: &str, command: &str) -> Option<u64> {
        let state = self.worker.state.lock().unwrap();
        self.worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire)
            .then(|| state.command_epoch(plugin, command))
            .flatten()
    }

    /// Check a managed shortcut target against the latest effective worker publication.
    /// Hidden panels remain eligible; trust, startup and declared commands still gate execution.
    pub(crate) fn shortcut_available(&self, plugin: &str, command: &str) -> bool {
        self.shortcut_epoch(plugin, command).is_some()
    }

    /// Accept a resolved plugin target through the existing checked command and panel route.
    /// Returns false when unavailable; true schedules dispatch, not successful guest execution.
    pub(crate) fn invoke_shortcut_at_epoch(
        &mut self,
        plugin: &str,
        command: &str,
        expected_epoch: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.shortcut_epoch(plugin, command) != Some(expected_epoch) {
            return false;
        }
        let plugin = plugin.to_owned();
        let command = command.to_owned();
        let parent = self.parent.clone();
        window.defer(cx, move |window, cx| {
            let _ = parent.update(cx, |app, cx| {
                // A publication or trust change can occur between key resolution and UI dispatch.
                if !app.session_state.workspace_trusted
                    || !app
                        .extensions
                        .read(cx)
                        .shortcut_epoch(&plugin, &command)
                        .is_some_and(|epoch| epoch == expected_epoch)
                {
                    return;
                }
                let accepted = app.extensions.read(cx).enqueue_command(
                    &plugin,
                    &command,
                    serde_json::Value::Null,
                    Some(expected_epoch),
                    None,
                );
                match accepted {
                    Err(error) => {
                        app.status = error;
                        cx.notify();
                    }
                    Ok(epoch) => {
                        app.reveal_plugin_command_panel(&plugin, &command, epoch, window, cx);
                    }
                }
            });
        });
        true
    }

    /// Queue a declared command for an enabled plugin, including one with a hidden surface.
    /// Success means accepted by the worker; execution errors are published through plugin status.
    pub(crate) fn invoke_command(
        &self,
        plugin: &str,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), String> {
        self.enqueue_command(plugin, command, arguments, None, None)
            .map(|_| ())
    }

    /// Validate and enqueue under one publication lock; an explicit owner is never recaptured.
    /// Ordinary host commands capture the current owner here instead of borrowing a UI snapshot.
    pub(super) fn enqueue_command(
        &self,
        plugin: &str,
        command: &str,
        arguments: serde_json::Value,
        expected_epoch: Option<u64>,
        context: Option<protocol::commands::Context>,
    ) -> Result<u64, String> {
        let state = self.worker.state.lock().unwrap();
        let current = state.command_epoch(plugin, command);
        if !self
            .worker
            .trusted
            .load(std::sync::atomic::Ordering::Acquire)
            || current.is_none()
            || expected_epoch.is_some_and(|expected| current != Some(expected))
        {
            return Err(format!("插件尚未就绪或未启用：{plugin}"));
        }
        if serde_json::to_vec(&arguments)
            .map_err(|error| error.to_string())?
            .len()
            > 65536
        {
            return Err("插件命令参数不能超过 64 KiB".into());
        }
        let epoch = current.expect("admission checked command ownership");
        self.worker
            .tx
            .send(Work::Invoke {
                plugin: plugin.into(),
                command: command.into(),
                arguments,
                context,
                expected_epoch: epoch,
            })
            .map_err(|_| "插件后台服务不可用".to_owned())?;
        Ok(epoch)
    }
}

impl EditorApp {
    /// Hide only a panel declared by the requesting plugin and save its visibility preference.
    /// The plugin decides when to close its view; the host owns native dock layout and persistence.
    pub(crate) fn hide_plugin_panel(&mut self, plugin: &str, panel: &str, cx: &mut Context<Self>) {
        let key = format!("{plugin}/{panel}");
        let Some(panel) = self.plugin_panels.get(&key).cloned() else {
            return;
        };
        panel.update(cx, |panel, cx| {
            panel.hide();
            cx.notify();
        });
        self.session_state
            .plugin_panel_visibility
            .insert(key, false);
        self.persist_session();
        self.dock_area.update(cx, |_, cx| cx.notify());
        cx.notify();
    }

    /// Entry point for project run/build actions: queue a plugin command and reveal its first panel.
    /// This is asynchronous and never installs or enables a plugin on the caller's behalf.
    pub(crate) fn invoke_plugin_command(
        &mut self,
        plugin: &str,
        command: &str,
        arguments: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let epoch = self
            .extensions
            .read(cx)
            .enqueue_command(plugin, command, arguments, None, None)?;
        self.reveal_plugin_command_panel(plugin, command, epoch, window, cx);
        Ok(())
    }

    /// Reveal only after command admission, sharing the ordinary command layout and visibility path.
    pub(super) fn reveal_plugin_command_panel(
        &mut self,
        plugin: &str,
        command: &str,
        expected_epoch: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Reconcile freshly published panels so callers do not depend on a previous repaint.
        self.extensions.update(cx, |owner, cx| owner.poll(cx));
        self.sync_plugin_panels(window, cx);
        // Polling may observe a replacement after admission. Pin its publication while revealing
        // so an old accepted command cannot send panel.opened to the replacement's surface.
        let worker = self.extensions.read(cx).worker.clone();
        let state = worker.state.lock().unwrap();
        if !worker.trusted.load(std::sync::atomic::Ordering::Acquire)
            || state.command_epoch(plugin, command) != Some(expected_epoch)
        {
            return;
        }
        let panel = self
            .extensions
            .read(cx)
            .entries
            .iter()
            .find(|entry| entry.manifest.id == plugin)
            .and_then(|entry| entry.manifest.panels.first())
            .and_then(|panel| self.plugin_panels.get(&format!("{plugin}/{}", panel.id)))
            .cloned();
        if let Some(panel) = panel {
            panel.update(cx, |panel, cx| {
                panel.show(window, cx);
                cx.notify();
            });
            self.dock_area.update(cx, |_, cx| cx.notify());
        }
        drop(state);
        cx.notify();
    }
}
