use std::collections::BTreeMap;

/// Stable command identifiers shared by buttons, shortcuts, and a future palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CommandId {
    SaveDocument,
    RefreshWorkspace,
    ToggleBottomPanel,
    ToggleSoftWrap,
    ToggleTheme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBindingDescriptor {
    pub keys: &'static str,
    pub context: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDescriptor {
    pub id: CommandId,
    pub title: &'static str,
    pub category: &'static str,
    pub default_binding: Option<KeyBindingDescriptor>,
}

/// A small lookup interface hiding registration and conflict validation.
#[derive(Debug, Clone)]
pub struct CommandRegistry {
    commands: BTreeMap<CommandId, CommandDescriptor>,
}

impl CommandRegistry {
    pub fn editor_defaults() -> Self {
        let commands = [
            CommandDescriptor {
                id: CommandId::SaveDocument,
                title: "Save",
                category: "File",
                default_binding: Some(KeyBindingDescriptor {
                    keys: "ctrl-s",
                    context: "EditorShell",
                }),
            },
            CommandDescriptor {
                id: CommandId::RefreshWorkspace,
                title: "Refresh files",
                category: "Workspace",
                default_binding: Some(KeyBindingDescriptor {
                    keys: "ctrl-shift-r",
                    context: "EditorShell",
                }),
            },
            CommandDescriptor {
                id: CommandId::ToggleBottomPanel,
                title: "Toggle panel",
                category: "View",
                default_binding: Some(KeyBindingDescriptor {
                    keys: "ctrl-j",
                    context: "EditorShell",
                }),
            },
            CommandDescriptor {
                id: CommandId::ToggleSoftWrap,
                title: "Toggle soft wrap",
                category: "View",
                default_binding: None,
            },
            CommandDescriptor {
                id: CommandId::ToggleTheme,
                title: "Toggle theme",
                category: "View",
                default_binding: Some(KeyBindingDescriptor {
                    keys: "ctrl-alt-t",
                    context: "EditorShell",
                }),
            },
        ]
        .into_iter()
        .map(|command| (command.id, command))
        .collect();

        Self { commands }
    }

    pub fn get(&self, id: CommandId) -> Option<&CommandDescriptor> {
        self.commands.get(&id)
    }

    pub fn all(&self) -> impl Iterator<Item = &CommandDescriptor> {
        self.commands.values()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn default_keybindings_do_not_conflict() {
        let registry = CommandRegistry::editor_defaults();
        let mut seen = BTreeSet::new();

        for binding in registry
            .all()
            .filter_map(|command| command.default_binding.as_ref())
        {
            assert!(seen.insert((binding.context, binding.keys)));
        }
    }
}
