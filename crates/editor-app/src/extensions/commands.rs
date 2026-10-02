//! Host features invoke installed plugin commands without depending on their implementation.
use super::*;

impl ExtensionPanel {
    /// Queue a declared command for an enabled plugin, including one with a hidden surface.
    /// Success means accepted by the worker; execution errors are published through plugin status.
    pub(crate) fn invoke_command(
        &self,
        plugin: &str,
        command: &str,
        arguments: serde_json::Value,
    ) -> Result<(), String> {
        let state = self.worker.state.lock().unwrap();
        let entry = state
            .entries
            .iter()
            .find(|entry| entry.manifest.id == plugin)
            .ok_or_else(|| format!("插件未安装：{plugin}"))?;
        if !entry.enabled || entry.error.is_some() || state.startup.contains_key(plugin) {
            return Err(format!("插件尚未就绪或未启用：{plugin}"));
        }
        if !entry
            .manifest
            .commands
            .iter()
            .any(|item| item.id == command)
        {
            return Err(format!("插件未声明命令：{plugin} / {command}"));
        }
        if serde_json::to_vec(&arguments)
            .map_err(|error| error.to_string())?
            .len()
            > 65536
        {
            return Err("插件命令参数不能超过 64 KiB".into());
        }
        drop(state);
        self.worker
            .tx
            .send(Work::Invoke {
                plugin: plugin.into(),
                command: command.into(),
                arguments,
            })
            .map_err(|_| "插件后台服务不可用".into())
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
        self.extensions
            .read(cx)
            .invoke_command(plugin, command, arguments)?;
        // Reconcile freshly published panels so callers do not depend on a previous repaint.
        self.extensions.update(cx, |owner, cx| owner.poll(cx));
        self.sync_plugin_panels(window, cx);
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
        cx.notify();
        Ok(())
    }
}
