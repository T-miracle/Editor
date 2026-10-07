//! Describes real command targets without changing their dispatch or lifetime.

use gpui_kit::{
    Action, App, KeyBinding, KeyBindingContextPredicate, KeyContext, Keystroke, SharedString,
};
use plugin_runtime::Installed;
use std::rc::Rc;

/// The tab reflects the handler's scope; it never grants broader dispatch authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Scope {
    Panel,
    Global,
}

/// Retain the exact action payload and predicate so later rebinding preserves behavior.
pub(super) enum Target {
    Native {
        action: Box<dyn Action>,
        predicate: Option<Rc<KeyBindingContextPredicate>>,
        action_input: Option<SharedString>,
    },
    Plugin {
        plugin: String,
        command: String,
    },
}

impl Clone for Target {
    fn clone(&self) -> Self {
        match self {
            Self::Native {
                action,
                predicate,
                action_input,
            } => Self::Native {
                action: action.boxed_clone(),
                predicate: predicate.clone(),
                action_input: action_input.clone(),
            },
            Self::Plugin { plugin, command } => Self::Plugin {
                plugin: plugin.clone(),
                command: command.clone(),
            },
        }
    }
}

/// One stable command variant can have several default one- or two-step bindings.
pub(super) struct Operation {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) scope: Scope,
    pub(super) target: Target,
    pub(super) defaults: Vec<Vec<String>>,
}

impl Clone for Operation {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            title: self.title.clone(),
            scope: self.scope,
            target: self.target.clone(),
            defaults: self.defaults.clone(),
        }
    }
}

/// Build a read-only snapshot from defaults, captured handlers, contexts and valid packages.
///
/// `available` must be captured before opening the modal. Include binding action clones
/// accepted by `Window::is_action_available_in`, because `available_actions` alone omits
/// actions that cannot be default-constructed, such as parameterized input Enter actions.
/// No package is started and no permission is granted while reading this catalog.
pub(super) fn operations(
    bindings: &[KeyBinding],
    available: &[Box<dyn Action>],
    contexts: &[KeyContext],
    plugins: &[Installed],
    cx: &App,
) -> Vec<Operation> {
    let mut result = Vec::<Operation>::new();
    for (index, binding) in bindings.iter().enumerate() {
        // GPUI resolves an identical key/context pair from newest to oldest. In particular,
        // upstream replaces Ctrl+Backspace's generic deletion with word deletion on Windows.
        if bindings[index + 1..].iter().any(|later| {
            later.keystrokes() == binding.keystrokes() && later.predicate() == binding.predicate()
        }) {
            continue;
        }
        let action = binding.action();
        if !available
            .iter()
            .any(|candidate| candidate.name() == action.name())
        {
            continue;
        }
        let predicate = binding.predicate();
        // Base Root owns window-wide focus traversal and copying selected UI text. These
        // handlers remain global; sharing Copy's action type does not make them Input commands.
        let scope = if shell_action(action.name())
            || predicate.is_none()
            || predicate
                .as_ref()
                .is_some_and(|value| value.to_string() == "Root")
        {
            Scope::Global
        } else {
            Scope::Panel
        };
        if scope == Scope::Panel
            && !predicate
                .as_ref()
                .is_some_and(|value| value.depth_of(contexts).is_some())
        {
            continue;
        }
        let Some(id) = native_id(
            action,
            binding.action_input().as_ref(),
            predicate.as_deref(),
            cx,
        ) else {
            // Unknown opaque payloads need explicit metadata; never merge distinct actions by name.
            continue;
        };
        let keys = binding
            .keystrokes()
            .iter()
            .map(|key| key.unparse())
            .collect::<Vec<_>>();
        if let Some(existing) = result.iter_mut().find(|operation| operation.id == id) {
            if !existing.defaults.contains(&keys) {
                existing.defaults.push(keys);
            }
            continue;
        }
        result.push(Operation {
            id,
            title: native_title(action),
            scope,
            target: Target::Native {
                action: action.boxed_clone(),
                predicate,
                action_input: binding.action_input(),
            },
            defaults: vec![keys],
        });
    }
    append_unbound(&mut result, available, contexts, cx);
    append_plugins(&mut result, plugins);
    result.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.id.cmp(&right.id))
    });
    result
}

/// Only handlers registered on the application's EditorShell count as application commands.
fn shell_action(name: &str) -> bool {
    matches!(
        name,
        "me_editor::SaveDocument"
            | "me_editor::RefreshWorkspace"
            | "me_editor::ToggleTheme"
            | "me_editor::NavigateToDefinition"
            | "me_editor::ShowDefinitionDetails"
            | "me_editor::NextSyntaxError"
            | "me_editor::PreviousSyntaxError"
            | "extensions::ToggleExtensions"
            | "extensions::QuitEditor"
            | "shortcuts::OpenShortcuts"
    )
}

/// Use verified handler metadata for unbound operations, never a transient captured stack.
fn append_unbound(
    result: &mut Vec<Operation>,
    available: &[Box<dyn Action>],
    contexts: &[KeyContext],
    cx: &App,
) {
    for action in available {
        if result.iter().any(|operation| {
            matches!(&operation.target,
            Target::Native { action: existing, .. } if existing.partial_eq(action.as_ref()))
        }) {
            continue;
        }
        let (scope, context) = if shell_action(action.name()) {
            (Scope::Global, "EditorShell")
        } else if matches!(
            action.name(),
            "input::ActivateToken"
                | "input::DeleteToBeginningOfLine"
                | "input::DeleteToEndOfLine"
                | "input::ShowCharacterPalette"
                | "input::GoToDefinition"
        ) {
            // Both EditorMode and ordinary InputMode use the public Input key context.
            // Captured handler availability distinguishes editor-only GoToDefinition.
            (Scope::Panel, "Input")
        } else {
            continue;
        };
        let predicate =
            Rc::new(KeyBindingContextPredicate::parse(context).expect("static handler context"));
        if scope == Scope::Panel && predicate.depth_of(contexts).is_none() {
            continue;
        }
        let Some(id) = native_id(action.as_ref(), None, Some(&predicate), cx) else {
            continue;
        };
        result.push(Operation {
            id,
            title: native_title(action.as_ref()),
            scope,
            target: Target::Native {
                action: action.boxed_clone(),
                predicate: Some(predicate),
                action_input: None,
            },
            defaults: Vec::new(),
        });
    }
}

/// Identity includes parameters and original scope; equal labels never collapse distinct targets.
fn native_id(
    action: &dyn Action,
    input: Option<&SharedString>,
    predicate: Option<&KeyBindingContextPredicate>,
    cx: &App,
) -> Option<String> {
    let parameters = if let Some(enter) = action.as_any().downcast_ref::<gpui_base::input::Enter>()
    {
        // Enter is no_json upstream: KeyBinding::action_input cannot distinguish its variants.
        serde_json::json!({"secondary": enter.secondary, "shift": enter.shift})
    } else if let Some(input) = input {
        serde_json::from_str(input.as_ref()).ok()?
    } else if cx
        .build_action(action.name(), None)
        .is_ok_and(|default| default.partial_eq(action))
    {
        serde_json::Value::Null
    } else {
        return None;
    };
    Some(format!(
        "native:{}",
        serde_json::json!([
            action.name(),
            parameters,
            predicate.map(ToString::to_string)
        ])
    ))
}

/// Manifest command shortcuts are currently dispatched at the shell, independent of panel titles.
fn append_plugins(result: &mut Vec<Operation>, plugins: &[Installed]) {
    for entry in plugins
        .iter()
        .filter(|entry| entry.enabled && entry.error.is_none())
    {
        for command in &entry.manifest.commands {
            let defaults = command
                .shortcut
                .as_ref()
                .and_then(|shortcut| {
                    shortcut
                        .split_whitespace()
                        .map(|part| Keystroke::parse(part).map(|key| key.unparse()))
                        .collect::<Result<Vec<_>, _>>()
                        .ok()
                        .filter(|keys| !keys.is_empty())
                })
                .into_iter()
                .collect();
            result.push(Operation {
                id: format!(
                    "plugin:{}",
                    serde_json::json!([entry.manifest.id, command.id])
                ),
                title: command.title.clone(),
                scope: Scope::Global,
                target: Target::Plugin {
                    plugin: entry.manifest.id.clone(),
                    command: command.id.clone(),
                },
                defaults,
            });
        }
    }
}

/// Translate known user operations; retain readable names for unrecognized upstream actions.
fn native_title(action: &dyn Action) -> String {
    let short_name = action.name().rsplit("::").next().unwrap_or(action.name());
    let key = format!("shortcuts.operation.{short_name}");
    let translated = rust_i18n::t!(key.as_str()).to_string();
    let title = if translated == key {
        let mut title = String::new();
        for (index, letter) in short_name.chars().enumerate() {
            if index > 0 && letter.is_uppercase() {
                title.push(' ');
            }
            title.push(letter);
        }
        title
    } else {
        translated
    };
    if let Some(enter) = action.as_any().downcast_ref::<gpui_base::input::Enter>() {
        let variant = match (enter.secondary, enter.shift) {
            (false, false) => "shortcuts.operation.enter_primary",
            (false, true) => "shortcuts.operation.enter_shift",
            (true, false) => "shortcuts.operation.enter_secondary",
            (true, true) => "shortcuts.operation.enter_secondary_shift",
        };
        return rust_i18n::t!(variant).to_string();
    }
    title
}
