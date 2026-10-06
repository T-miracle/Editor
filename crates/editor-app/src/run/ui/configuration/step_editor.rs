//! Structured prepared-action editing over native fields, without shell command-line parsing.
use super::*;
use editor_core::{RunStep, RunTarget, StepTarget};

/// Action type is explicit; changing the selector preserves values in the other native fields.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum ActionKind {
    Program,
    Script,
    Reference,
}

/// A transient action editor owns a copy; cancellation cannot change the original list row.
pub(in crate::run::ui) struct StepEditor {
    kind: ActionKind,
    name: Entity<InputState>,
    program: Entity<InputState>,
    arguments: Entity<TextareaState>,
    script: Entity<TextareaState>,
    pub(super) reference: Entity<InputState>,
    original_arguments: Vec<String>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}
impl StepEditor {
    /// Initialize all native states once from a typed action; opaque provider actions never enter here.
    pub(in crate::run::ui) fn new(
        step: Option<RunStep>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let step = step.unwrap_or(RunStep {
            name: String::new(),
            target: StepTarget::Action {
                target: RunTarget::Program {
                    program: String::new(),
                    args: vec![],
                },
            },
        });
        let (kind, program, args, script, reference) = match step.target {
            StepTarget::Build { config } => (
                ActionKind::Reference,
                String::new(),
                vec![],
                String::new(),
                config,
            ),
            StepTarget::Action {
                target:
                    RunTarget::Script {
                        interpreter,
                        args,
                        script,
                    },
            } => (ActionKind::Script, interpreter, args, script, String::new()),
            StepTarget::Action {
                target: RunTarget::Program { program, args },
            } => (
                ActionKind::Program,
                program,
                args,
                String::new(),
                String::new(),
            ),
            StepTarget::Action {
                target: RunTarget::Provided { .. },
            } => unreachable!("provider preparations are read-only"),
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(step.name));
        let program = cx.new(|cx| InputState::new(window, cx).default_value(program));
        let arguments = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .default_value(args.join("\n"))
        });
        let script = cx.new(|cx| TextareaState::new(window, cx).rows(3).default_value(script));
        let reference = cx.new(|cx| InputState::new(window, cx).default_value(reference));
        let mut subscriptions = Vec::new();
        for input in [&name, &program, &reference] {
            subscriptions.push(cx.subscribe(input, |state, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    state.error = None;
                    cx.notify();
                }
            }));
        }
        for input in [&arguments, &script] {
            subscriptions.push(cx.subscribe(input, |state, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    state.error = None;
                    cx.notify();
                }
            }));
        }
        Self {
            kind,
            name,
            program,
            arguments,
            script,
            reference,
            original_arguments: args,
            error: None,
            _subscriptions: subscriptions,
        }
    }

    /// Focus the first native field when opening a new or existing structured action.
    pub(super) fn focus_handle(&self, cx: &gpui_kit::App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }

    /// Encode an accepted typed row through the existing lossless storage representation.
    /// Unchanged argv keeps even empty/multiline entries; editing interprets one literal item per line.
    pub(super) fn row_text(&self, cx: &gpui_kit::App) -> Result<String, String> {
        if self.kind == ActionKind::Script && self.script.read(cx).value().trim().is_empty() {
            return Err(t!("run.form_script_required").into());
        }
        let text = self.arguments.read(cx).value().to_string();
        let args = if text == self.original_arguments.join("\n") {
            self.original_arguments.clone()
        } else {
            text.lines()
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect()
        };
        let target = match self.kind {
            ActionKind::Program => StepTarget::Action {
                target: RunTarget::Program {
                    program: self.program.read(cx).value().trim().into(),
                    args,
                },
            },
            ActionKind::Script => StepTarget::Action {
                target: RunTarget::Script {
                    interpreter: self.program.read(cx).value().trim().into(),
                    args,
                    script: self.script.read(cx).value().into(),
                },
            },
            ActionKind::Reference => StepTarget::Build {
                config: self.reference.read(cx).value().into(),
            },
        };
        let step = RunStep {
            name: self.name.read(cx).value().to_string(),
            target,
        };
        step.validate().map_err(|error| error.to_string())?;
        let row = crate::run::render_steps(&[step]);
        crate::run::parse_steps(&row)?;
        Ok(row)
    }

    /// Report a rejected detail edit without closing it or changing another list's rows.
    pub(super) fn reject(&mut self, message: String, cx: &mut Context<Self>) {
        self.error = Some(message);
        cx.notify();
    }

    /// Three explicit types share native fields; reference names follow the current public contract.
    pub(super) fn render_form(
        state: &Entity<Self>,
        app: &Entity<EditorApp>,
        cx: &gpui_kit::App,
    ) -> AnyElement {
        let editor = state.read(cx);
        let owner = state.clone();
        let mut fields = v_flex()
            .gap_3()
            .child(labeled(
                "run-step-name",
                t!("run.field_name").into(),
                crate::ui::controls::Input::new(&editor.name).into_any_element(),
            ))
            .child(
                div().debug_selector(|| "run-step-kind".into()).child(
                    crate::ui::controls::SegmentedTabs::new("run-step-kind")
                        .labels([
                            t!("run.field_program"),
                            t!("run.form_shell"),
                            t!("run.form_build_reference"),
                        ])
                        .selected_index(match editor.kind {
                            ActionKind::Program => 0,
                            ActionKind::Script => 1,
                            ActionKind::Reference => 2,
                        })
                        .on_change(move |index, _, cx| {
                            owner.update(cx, |editor, cx| {
                                editor.kind = [
                                    ActionKind::Program,
                                    ActionKind::Script,
                                    ActionKind::Reference,
                                ][index];
                                editor.error = None;
                                cx.notify();
                            })
                        }),
                ),
            );
        if editor.kind == ActionKind::Reference {
            let label = editor.reference.read(cx).value().to_string();
            fields = fields
                .child(labeled(
                    "run-step-reference-field",
                    t!("run.form_reference_config").into(),
                    picker_button(
                        app,
                        PickerKind::Reference,
                        if label.is_empty() {
                            t!("run.form_choose_target").into()
                        } else {
                            label
                        },
                        cx,
                    )
                    .into_any_element(),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(t!("run.form_reference_hint")),
                );
        } else {
            fields = fields
                .child(labeled(
                    "run-step-program",
                    if editor.kind == ActionKind::Script {
                        t!("run.field_interpreter")
                    } else {
                        t!("run.field_program")
                    }
                    .into(),
                    crate::ui::controls::Input::new(&editor.program).into_any_element(),
                ))
                .child(labeled(
                    "run-step-arguments",
                    t!("run.field_arguments").into(),
                    crate::ui::controls::Textarea::new(&editor.arguments).into_any_element(),
                ));
            if editor.kind == ActionKind::Script {
                fields = fields.child(labeled(
                    "run-step-script",
                    t!("run.field_script").into(),
                    crate::ui::controls::Textarea::new(&editor.script).into_any_element(),
                ));
            }
        }
        fields
            .when_some(editor.error.clone(), |fields, error| {
                fields.child(div().text_color(cx.theme().danger).child(error))
            })
            .into_any_element()
    }
}

/// Detail fields keep the same local input appearance and accessible native state as the main form.
fn labeled(id: &'static str, label: String, input: AnyElement) -> AnyElement {
    v_flex()
        .gap_2()
        .child(div().text_xs().child(label))
        .child(div().debug_selector(move || id.into()).child(input))
        .into_any_element()
}
