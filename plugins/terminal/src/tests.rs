//! Guest tests exercise the same event/snapshot interface without native OS resources.
use super::*;
thread_local! {static CALLS:RefCell<Vec<api::Operation>>=const{RefCell::new(vec![])};}
/// Model only SDK admission here; asynchronous completion is explicitly delivered by each test.
pub(super) fn host(request: api::Operation) -> Result<api::Value, api::Failure> {
    CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        let handle = request_handle(calls.len() as u64 + 1);
        let value = match &request {
            api::Operation::OpenData | api::Operation::OpenWorkspace => {
                return Err(api::Failure::new(
                    api::ErrorCode::NotFound,
                    "Missing fixture file",
                ));
            }
            api::Operation::Process {
                operation: process::Operation::Execute { .. },
            } => api::Value::Resource(handle),
            api::Operation::Editor { .. } => api::Value::Accepted(handle),
            _ => api::Value::Unit,
        };
        calls.push(request);
        Ok(value)
    })
}
/// Opaque fixture identities use the same instance and scope binding as production.
fn request_handle(resource: u64) -> api::ResourceHandle {
    api::ResourceHandle {
        instance: "terminal-test".into(),
        scope: "workspace-test".into(),
        resource,
    }
}
fn app() -> Terminal {
    CALLS.with(|c| c.borrow_mut().clear());
    let mut app = Terminal::prepare(
        Environment {
            workspace: "C:/project".into(),
            os: "windows".into(),
            background: 0xffffff,
            foreground: 0x101010,
            border: 0xcccccc,
            ..Environment::default()
        },
        None,
    )
    .unwrap();
    app.activate();
    app
}

/// Initial resize does not advertise scrollable blank rows; real output does.
#[test]
fn blank_history_and_real_history_have_distinct_scroll_ranges() {
    let mut app = app();
    app.event(Event::Resize {
        width: 1000.,
        height: 600.,
        cell_width: 8.,
        cell_height: 20.,
    });
    let handle = app.tabs[0].handle.clone().unwrap();
    app.event(Event::ProcessOutput {
        handle: handle.clone(),
        bytes: b"PS C:\\project> ".to_vec(),
    });
    assert!(app.scene().scroll.is_none());
    let output = (0..70).map(|i| format!("line{i}\r\n")).collect::<String>();
    app.event(Event::ProcessOutput {
        handle: handle.clone(),
        bytes: output.into_bytes(),
    });
    let scene = app.scene();
    let scroll = scene.scroll.unwrap();
    // An absent guest timeout keeps visibility under the host's shared scrollbar policy.
    assert!(scroll.content > app.height);
    app.event(Event::Scroll { offset: 0. });
    assert!(app.tabs[0].term.screen().scrollback() > 0);
}

/// Session identity, user names, cwd and history survive replacement without replaying commands.
#[test]
fn restore_recreates_shells_but_never_replays_old_input() {
    let mut app = app();
    app.add(0, "C:/second".into());
    assert_eq!(app.tabs[0].name, "powershell");
    assert_eq!(app.tabs[1].name, "powershell");
    let id = app.tabs[1].id;
    app.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: "sessions".into(),
        action: ui::Action::Rename {
            id: id.to_string(),
            value: "构建任务".into(),
        },
    }));
    let handle = app.tabs[1].handle.clone().unwrap();
    app.event(Event::ProcessOutput {
        handle: handle.clone(),
        bytes: b"OLD OUTPUT\r\n".to_vec(),
    });
    let snapshot = app.snapshot();
    CALLS.with(|c| c.borrow_mut().clear());
    let mut restored = Terminal::prepare(app.env.clone(), Some(snapshot)).unwrap();
    assert_eq!(restored.tabs[1].name, "构建任务");
    assert_eq!(restored.tabs[1].cwd, "C:/second");
    assert!(restored.snapshot().data.contains("OLD OUTPUT"));
    assert!(CALLS.with(|c| c.borrow().is_empty()));
    restored.activate();
    assert_eq!(
        CALLS.with(|c| c
            .borrow()
            .iter()
            .filter(|r| matches!(
                r,
                api::Operation::Process {
                    operation: process::Operation::Execute { .. }
                }
            ))
            .count()),
        2
    );
    assert!(!CALLS.with(|c| {
        c.borrow().iter().any(|r| {
            matches!(
                r,
                api::Operation::Process {
                    operation: process::Operation::Write { .. }
                }
            )
        })
    }));
}

/// Session controls belong to the native protocol; the guest paints only terminal content.
#[test]
fn terminal_delegates_tab_controls_to_canvas_controls() {
    let app = app();
    let scene = app.scene();
    assert!(matches!(app.document().root.kind, ui::Kind::Row { .. }));
    app.document().validate().unwrap();
    let sidebar = app.sessions();
    assert_eq!(sidebar.selected, Some(app.tabs[app.active].id.to_string()));
    assert_eq!(sidebar.items[0].label, app.tabs[0].name);
    assert!(
        !scene
            .paint
            .iter()
            .any(|p| matches!(p,Paint::Text{text,..} if text==&app.tabs[0].name))
    );
}

/// Moving the sidebar shifts rendering, selection and menu anchors while preserving the grid.
#[test]
fn left_tabs_keep_canvas_input_and_persistent_layout_in_sync() {
    assert_eq!(
        Settings::parse("{}").unwrap().tab_position,
        ui::SideTabsPosition::Right
    );
    let settings = Settings::parse(r#"{"tab_position":"left"}"#).unwrap();
    assert_eq!(settings.tab_position, ui::SideTabsPosition::Left);
    assert!(Settings::parse(r#"{"tab_position":"top"}"#).is_err());
    let mut terminal = app();
    terminal.event(Event::Resize {
        width: 1000.,
        height: 600.,
        cell_width: 8.,
        cell_height: 20.,
    });
    output(&mut terminal, b"SELECT ME");
    let dimensions = terminal.tabs[0].term.screen().size();
    terminal.settings.tab_position = ui::SideTabsPosition::Left;
    terminal.resize_grid();
    assert_eq!(terminal.tabs[0].term.screen().size(), dimensions);
    let scene = terminal.scene();
    assert_eq!(terminal.sessions().position, ui::SideTabsPosition::Left);
    assert_eq!(scene.caret.unwrap().x, 8. + 9. * terminal.cw);
    assert!(
        scene
            .paint
            .iter()
            .any(|paint| matches!(paint, Paint::Text { x, text, .. }
        if *x == 8. && text == "S"))
    );
    let x = terminal.content_left() + 8.;
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x,
        y: 8.,
        button: 0,
        clicks: 2,
        shift: false,
    });
    assert_eq!(terminal.selected_text().as_deref(), Some("SELECT"));
    terminal.event(Event::Pointer {
        kind: "up".into(),
        x,
        y: 8.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    // Session interactions arrive on their own node, outside canvas-local coordinates.
    let id = terminal.tabs[0].id.to_string();
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: "sessions".into(),
        action: ui::Action::Context { id, x: 12., y: 16. },
    }));
    assert_eq!(terminal.menu_position, (12., 16.));
    terminal.menu = None;
    output(
        &mut terminal,
        (0..70)
            .map(|i| format!("\r\nline{i}"))
            .collect::<String>()
            .as_bytes(),
    );
    let scroll = terminal.scene().scroll.unwrap();
    assert!(scroll.content > terminal.height);
    terminal.event(Event::Wheel {
        delta: 3.,
        shift: false,
        x: 10.,
        y: 30.,
    });
    let offset = terminal.tabs[0].term.screen().scrollback();
    assert!(offset > 0);
    terminal.event(Event::Wheel {
        delta: 3.,
        shift: false,
        x: 10.,
        y: 30.,
    });
    assert!(terminal.tabs[0].term.screen().scrollback() > offset);
    let restored = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    assert_eq!(restored.settings.tab_position, ui::SideTabsPosition::Left);
    assert_eq!(restored.tab_width, terminal.tab_width);
}

/// Cwd metadata remains intact even when ConPTY splits an escape across reads.
#[test]
fn shell_directory_metadata_can_cross_output_chunks() {
    let mut app = app();
    let handle = app.tabs[0].handle.clone().unwrap();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "C:/changed");
    let bytes = format!("\x1b]633;P;Cwd64={encoded}\x07").into_bytes();
    for chunk in bytes.chunks(3) {
        app.event(Event::ProcessOutput {
            handle: handle.clone(),
            bytes: chunk.to_vec(),
        });
    }
    assert_eq!(app.tabs[0].cwd, "C:/changed");
}

/// Feed the public output event using the active host resource handle.
fn output(app: &mut Terminal, bytes: &[u8]) {
    app.event(Event::ProcessOutput {
        handle: app.tabs[app.active].handle.clone().unwrap(),
        bytes: bytes.to_vec(),
    });
}

/// Observe bytes delivered to the host PTY, excluding setup and resize requests.
fn writes() -> Vec<u8> {
    CALLS.with(|calls| {
        calls
            .borrow()
            .iter()
            .filter_map(|r| match r {
                api::Operation::Process {
                    operation: process::Operation::Write { bytes, .. },
                } => Some(bytes.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    })
}

/// Chunked UTF-8 and truecolor output must survive parsing, painting and snapshot restoration.
#[test]
fn unicode_colors_and_legacy_snapshot_round_trip() {
    let mut terminal = app();
    let bytes = "\x1b[38;2;12;34;56m中文 e\u{301}\x1b[0m".as_bytes();
    for byte in bytes {
        output(&mut terminal, &[*byte]);
    }
    let screen = terminal.tabs[0].term.screen();
    assert_eq!(screen.cell(0, 0).unwrap().contents(), "中");
    assert!(screen.cell(0, 1).unwrap().is_wide_continuation());
    assert_eq!(
        screen.cell(0, 0).unwrap().fgcolor(),
        emulator::Color::Rgb(12, 34, 56)
    );
    assert!(
        terminal
            .scene()
            .paint
            .iter()
            .any(|p| matches!(p, Paint::Text { text, color: 0x0c2238, .. } if text == "中"))
    );
    let snapshot = terminal.snapshot();
    assert_eq!(snapshot.schema, 2);
    let restored = Terminal::prepare(terminal.env.clone(), Some(snapshot)).unwrap();
    assert!(
        restored.tabs[0]
            .term
            .screen()
            .contents()
            .contains("中文 e\u{301}")
    );
    assert_eq!(
        restored.tabs[0].term.screen().cell(0, 0).unwrap().fgcolor(),
        emulator::Color::Rgb(12, 34, 56)
    );
    assert!(
        writes().is_empty(),
        "restoring old output must not execute shell input"
    );
}

/// Named, bold, dim and default SGR text use distinct plugin palette entries.
#[test]
fn sgr_text_uses_named_intensity_and_default_theme_colors() {
    let mut terminal = app();
    output(
        &mut terminal,
        b"\x1b[31mR\x1b[0m\x1b[1;31mB\x1b[0m\x1b[2;31mD\x1b[0m\x1b[2mM\x1b[0m",
    );
    let screen = terminal.tabs[0].term.screen();
    for (column, index) in [(0, 1), (1, 9), (2, 260), (3, 268)] {
        assert_eq!(
            screen.cell(0, column).unwrap().fgcolor(),
            emulator::Color::Idx(index)
        );
    }
    let scene = terminal.scene();
    for (glyph, index) in [('R', 1), ('B', 9), ('D', 260), ('M', 268)] {
        assert!(scene.paint.iter().any(|paint| matches!(
            paint,
            Paint::Text { text, color, .. } if text == &glyph.to_string() && *color == terminal.color(index)
        )));
    }
}

/// Theme tokens cover every named color and take precedence over private settings.
#[test]
fn editor_theme_can_override_every_terminal_palette_role() {
    let mut terminal = app();
    let names = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    ];
    let mut environment = terminal.env.clone();
    for (index, name) in names.iter().enumerate() {
        environment
            .theme_colors
            .insert(format!("terminal.ansi.{name}"), 0x112200 + index as u32);
    }
    for (index, name) in names[..8].iter().enumerate() {
        environment
            .theme_colors
            .insert(format!("terminal.ansi.dim_{name}"), 0x223300 + index as u32);
    }
    for (name, value) in [
        ("foreground", 0x334401),
        ("background", 0x334402),
        ("cursor", 0x334403),
        ("cursor_text", 0x334404),
        ("selection", 0x334405),
        ("bright_foreground", 0x334406),
        ("dim_foreground", 0x334407),
        ("indexed.200", 0x334408),
    ] {
        environment
            .theme_colors
            .insert(format!("terminal.{name}"), value);
    }
    terminal.event(Event::Theme(environment));
    for index in 0..16 {
        assert_eq!(terminal.color(index), 0x112200 + index as u32);
    }
    for index in 0..8 {
        assert_eq!(terminal.color(index + 259), 0x223300 + index as u32);
    }
    for (index, value) in [
        (256, 0x334401),
        (257, 0x334402),
        (258, 0x334403),
        (267, 0x334406),
        (268, 0x334407),
        (200, 0x334408),
    ] {
        assert_eq!(terminal.color(index), value);
    }
    assert_eq!(terminal.cursor_text_color(), 0x334404);
    assert_eq!(terminal.selection_color(), 0x334405);
    terminal.settings.theme.foreground = Some("#ABCDEF".into());
    terminal.settings.theme.ansi = Some(std::array::from_fn(|_| "#123456".into()));
    assert_eq!(terminal.color(256), 0x334401);
    assert_eq!(terminal.color(1), 0x112201);
}

#[test]
fn live_theme_updates_terminal_text_styles() {
    let mut terminal = app();
    let handle = terminal.tabs[0].handle.clone();
    let mut environment = terminal.env.clone();
    environment.ui_font = FontStyle {
        family: Some("Theme UI".into()),
        size_px: Some(15.),
        bold: None,
    };
    environment.mono_font = FontStyle {
        family: Some("Theme Mono".into()),
        size_px: Some(17.),
        bold: None,
    };
    environment.theme_text_styles.insert(
        "terminal.tab".into(),
        FontStyle {
            family: Some("Theme Tab".into()),
            size_px: Some(19.),
            bold: Some(true),
        },
    );
    terminal.event(Event::Theme(environment));
    let scene = terminal.scene();
    assert_eq!(scene.font.family.as_deref(), Some("Theme Mono"));
    assert_eq!(scene.font.size_px, Some(17.));
    assert!(!scene.paint.iter().any(|paint| matches!(
        paint,
        Paint::Text { font: Some(family), size: 19., bold: true, .. } if family == "Theme Tab"
    )));
    terminal.rename = Some(terminal.tabs[0].id);
    let sidebar = terminal.sessions();
    assert_eq!(sidebar.rename, Some(terminal.tabs[0].id.to_string()));
    assert_eq!(terminal.tabs[0].handle, handle);
}

/// Installed editor themes arrive through the same API and override only declared keys.
#[test]
fn partial_external_theme_keeps_plugin_ansi_fallbacks() {
    let mut terminal = app();
    let mut environment = terminal.env.clone();
    environment.dark = true;
    environment.background = 0x101820;
    environment.foreground = 0xe0e8f0;
    environment
        .theme_colors
        .insert("terminal.ansi.red".into(), 0xf08070);
    terminal.event(Event::Theme(environment));

    assert_eq!(terminal.color(1), 0xf08070);
    assert_eq!(terminal.color(2), 0x82c991);
    assert_eq!(terminal.color(256), 0xe0e8f0);
    assert_eq!(terminal.color(257), 0x101820);

    let mut next = terminal.env.clone();
    next.theme_colors.clear();
    terminal.event(Event::Theme(next));
    assert_eq!(terminal.color(1), 0xff7673);
}

/// User tokens override owned control defaults; undeclared roles retain generic theme colors.
#[test]
fn external_theme_overrides_terminal_window_colors() {
    let mut terminal = app();
    let cwd = terminal.env.workspace.clone();
    terminal.add(0, cwd);
    terminal.menu = Some(TerminalMenu::Commands);
    terminal.error = Some("theme error".into());
    let mut environment = terminal.env.clone();
    environment.muted = 0xeeeeee;
    let roles = [
        "tab_bar.background",
        "tab_bar.border",
        "tab.border",
        "tab.active.background",
        "tab.active.foreground",
        "tab.inactive.background",
        "tab.inactive.foreground",
        "tab.close.background",
        "tab.close.foreground",
        "menu.background",
        "menu.foreground",
        "error.foreground",
    ];
    for (index, role) in roles.iter().enumerate() {
        environment
            .theme_colors
            .insert(format!("terminal.ui.{role}"), 0x123400 + index as u32);
    }
    terminal.event(Event::Theme(environment));
    let scene = terminal.scene();
    for (index, role) in roles.iter().enumerate() {
        let expected = 0x123400 + index as u32;
        assert_eq!(
            scene.paint.iter().any(|paint| match paint {
                Paint::Fill { color, .. } | Paint::Text { color, .. } => *color == expected,
                // Vector operations belong to other plugins and carry their colors in SVG source.
                Paint::Svg { .. } => false,
            }),
            role == &"error.foreground",
            "missing {role}"
        );
    }

    let mut next = terminal.env.clone();
    next.theme_colors.clear();
    terminal.event(Event::Theme(next));
    assert_eq!(
        terminal.ui_color("tab_bar.background", terminal.env.muted),
        0xeeeeee
    );
    let mut dark = terminal.env.clone();
    dark.dark = true;
    terminal.event(Event::Theme(dark));
    assert_eq!(
        terminal.ui_color("tab_bar.background", terminal.env.muted),
        0x2b2d30
    );
}

#[test]
fn native_sidebar_events_keep_session_identity_and_resize_pty() {
    let mut terminal = app();
    terminal.add(0, "C:/second".into());
    let first = terminal.tabs[0].id.to_string();
    let second = terminal.tabs[1].id.to_string();
    let send = |terminal: &mut Terminal, action| {
        terminal.event(Event::Ui(ui::UiEvent {
            revision: 0,
            node: "sessions".into(),
            action,
        }))
    };
    send(&mut terminal, ui::Action::Select(first.clone()));
    assert_eq!(terminal.active, 0);
    send(
        &mut terminal,
        ui::Action::Move {
            from: first.clone(),
            to: second.clone(),
        },
    );
    assert_eq!(terminal.tabs[1].id.to_string(), first);
    send(
        &mut terminal,
        ui::Action::Rename {
            id: first.clone(),
            value: "构建".into(),
        },
    );
    assert_eq!(terminal.tabs[1].name, "构建");
    let columns = terminal.tabs[1].term.screen().size().1;
    send(&mut terminal, ui::Action::Resize(300.));
    // Native layout publishes the resulting canvas extent separately from the divider event.
    terminal.event(Event::Resize {
        width: 600.,
        height: 420.,
        cell_width: 8.,
        cell_height: 20.,
    });
    assert!(terminal.tabs[1].term.screen().size().1 < columns);
    send(&mut terminal, ui::Action::Close(second));
    assert_eq!(terminal.tabs[terminal.active].id.to_string(), first);
    send(
        &mut terminal,
        ui::Action::Context {
            id: first,
            x: 12.,
            y: 24.,
        },
    );
    assert!(terminal.menu.is_some());
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: "terminal-menu".into(),
        action: ui::Action::Dismiss,
    }));
    assert!(terminal.menu.is_none());
    terminal.document().validate().unwrap();
}

/// Every close entry point hides the panel only after its last session and process are removed.
#[test]
fn final_tab_close_requests_panel_hiding_for_all_close_actions() {
    for action in ["tab", "shortcut", "command"] {
        let mut terminal = app();
        terminal.add(0, terminal.env.workspace.clone());
        let first = terminal.tabs[0].id;
        let last = terminal.tabs[1].id;
        let last_handle = terminal.tabs[1].handle.clone().unwrap();
        CALLS.with(|calls| calls.borrow_mut().clear());
        terminal.event(Event::Ui(ui::UiEvent {
            revision: 0,
            node: "sessions".into(),
            action: ui::Action::Close(first.to_string()),
        }));
        assert_eq!(terminal.tabs.len(), 1);
        assert_eq!(terminal.tabs[terminal.active].id, last);
        assert!(
            !CALLS.with(|calls| calls.borrow().iter().any(|request| matches!(
                request,
                api::Operation::Editor {
                    operation: api::EditorOperation::SetPanelVisibility { visible: false, .. },
                    ..
                }
            )))
        );
        CALLS.with(|calls| calls.borrow_mut().clear());
        terminal.menu = Some(TerminalMenu::Commands);
        terminal.rename = Some(last);
        let event = match action {
            "tab" => Event::Ui(ui::UiEvent {
                revision: 0,
                node: "sessions".into(),
                action: ui::Action::Close(last.to_string()),
            }),
            "shortcut" => Event::Key {
                key: "w".into(),
                ctrl: true,
                shift: true,
                alt: false,
            },
            _ => Event::Command {
                id: "terminal.close".into(),
                cwd: None,
                text: None,
                arguments: None,
            },
        };
        terminal.event(event);
        assert!(terminal.tabs.is_empty(), "{action}");
        assert!(terminal.menu.is_none());
        assert!(terminal.rename.is_none());
        assert!(terminal.sessions().items.is_empty());
        CALLS.with(|calls| {
            let requests = calls.borrow();
            // Process retirement precedes panel hiding and must not launch a replacement shell.
            assert!(
                matches!(&requests[..], [api::Operation::Process { operation: process::Operation::Terminate { handle } }, api::Operation::Editor { operation: api::EditorOperation::SetPanelVisibility {panel,visible:false}, .. }]
                if *handle == last_handle && panel == "terminal"),
                "{action}: {requests:?}"
            );
        });
        CALLS.with(|calls| calls.borrow_mut().clear());
        terminal.close(0);
        assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    }
}

/// Reopening a closed panel starts one default shell and preserves existing or pending sessions.
#[test]
fn opening_empty_terminal_creates_one_default_tab() {
    let mut terminal = app();
    terminal.close(0);
    CALLS.with(|calls| calls.borrow_mut().clear());
    let opened = || Event::Command {
        id: "panel.opened".into(),
        cwd: None,
        text: None,
        arguments: None,
    };
    terminal.event(opened());
    assert_eq!(terminal.tabs.len(), 1);
    assert_eq!(terminal.tabs[0].name, "powershell");
    assert_eq!(terminal.tabs[0].cwd, terminal.env.workspace);
    let handle = terminal.tabs[0].handle.clone();
    terminal.event(opened());
    terminal.event(Event::Focus(true));
    assert_eq!(terminal.tabs.len(), 1);
    assert_eq!(terminal.tabs[0].handle, handle);
    assert_eq!(
        CALLS.with(|calls| calls
            .borrow()
            .iter()
            .filter(|request| matches!(
                request,
                api::Operation::Process {
                    operation: process::Operation::Execute { .. }
                }
            ))
            .count()),
        1
    );
    terminal.close(0);
    terminal.pending_editor.insert(
        999,
        commands::PendingEditor::BeforeSave(commands::OpenOptions::default()),
    );
    CALLS.with(|calls| calls.borrow_mut().clear());
    terminal.event(opened());
    assert!(terminal.tabs.is_empty());
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
}

/// A live editor mode change recolors existing terminal cells without recreating the Shell.
#[test]
fn live_dark_mode_recolors_existing_output() {
    let mut terminal = app();
    output(&mut terminal, b"\x1b[33mY");
    let handle = terminal.tabs[0].handle.clone();
    assert_eq!(terminal.color(3), 0x8a5a00);
    let mut environment = terminal.env.clone();
    environment.dark = true;
    environment.background = 0x1e1f22;
    environment.foreground = 0xdfe1e5;
    terminal.event(Event::Theme(environment));
    assert_eq!(terminal.tabs[0].handle, handle);
    assert_eq!(terminal.color(3), 0xd7ba7d);
    assert!(terminal.scene().paint.iter().any(|paint| matches!(
        paint,
        Paint::Text { text, color: 0xd7ba7d, .. } if text == "Y"
    )));
}

/// The default caret is a narrow beam, while a Shell-requested block is honored.
#[test]
fn default_caret_is_beam_and_shell_can_request_block() {
    let mut terminal = app();
    assert_eq!(terminal.tabs[0].term.cursor_shape(), 5);
    let cursor = terminal.scene().caret.unwrap();
    assert!(terminal.scene().paint.iter().any(|paint| matches!(
        paint,
        Paint::Fill { rect, color, .. }
            if rect.x == cursor.x && rect.y == cursor.y && rect.w == 2.
                && rect.h == cursor.h && *color == terminal.color(258)
    )));
    output(&mut terminal, b"\x1b[2 q");
    assert_eq!(terminal.tabs[0].term.cursor_shape(), 1);
    output(&mut terminal, b"\x1b[0 q");
    assert_eq!(terminal.tabs[0].term.cursor_shape(), 5);
}

/// Blank rows and trailing grid cells cannot start or visually extend local selection.
#[test]
fn selection_starts_only_within_printed_line_content() {
    let mut terminal = app();
    output(&mut terminal, b"hello world");
    for (x, y) in [(200., 9.), (9., 8. + terminal.ch * 3.), (1., 9.)] {
        terminal.event(Event::Pointer {
            kind: "down".into(),
            x,
            y,
            button: 0,
            clicks: 1,
            shift: false,
        });
        terminal.event(Event::Pointer {
            kind: "move".into(),
            x: x + 30.,
            y,
            button: 0,
            clicks: 1,
            shift: false,
        });
        assert!(terminal.tabs[0].term.selected_range().is_none());
    }
    // A printed line may contain a space between words, which remains selectable.
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 8. + terminal.cw * 5. + 1.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert!(terminal.tabs[0].term.selected_range().is_some());
    terminal.event(Event::Pointer {
        kind: "move".into(),
        x: 300.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    let line_end = 8. + terminal.cw * 11.;
    assert!(!terminal.scene().paint.iter().any(|paint| matches!(
        paint,
        Paint::Fill { rect, color, .. }
            if rect.y == 8. && rect.x >= line_end && rect.x < terminal.content_right()
                && *color == terminal.selection_color()
    )));
}

/// TUI mouse reporting still receives presses on blank cells before local hit testing.
#[test]
fn blank_cells_still_report_mouse_to_tui() {
    let mut terminal = app();
    output(&mut terminal, b"\x1b[?1000h\x1b[?1006h");
    CALLS.with(|calls| calls.borrow_mut().clear());
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 200.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert_eq!(writes(), b"\x1b[<0;23;1M");
    assert!(terminal.tabs[0].term.selected_range().is_none());
}

/// Application arrows, bracketed paste, status queries and focus events reach the PTY once.
#[test]
fn shell_input_modes_and_queries_are_forwarded() {
    let mut terminal = app();
    output(
        &mut terminal,
        b"\x1b[?1h\x1b[?2004h\x1b[?1004h\x1b[2;3H\x1b[6n\x1b[5n",
    );
    terminal.event(Event::Key {
        key: "up".into(),
        ctrl: false,
        alt: false,
        shift: false,
    });
    terminal.event(Event::Text("echo hello world".into()));
    terminal.event(Event::Paste("a\nb".into()));
    terminal.event(Event::Focus(true));
    assert_eq!(
        writes(),
        b"\x1b[2;3R\x1b[0n\x1bOAecho hello world\x1b[200~a\nb\x1b[201~\x1b[I"
    );
    CALLS.with(|calls| calls.borrow_mut().clear());
    output(&mut terminal, b"\x1b]10;?\x07");
    assert_eq!(writes(), b"\x1b]10;rgb:1010/1010/1010\x07");
}

/// Alternate-screen applications must not replace the primary history in saved sessions.
#[test]
fn alternate_screen_preserves_primary_history() {
    let mut terminal = app();
    output(
        &mut terminal,
        b"shell history\r\n\x1b[?1049h\x1b[2J\x1b[Hfullscreen",
    );
    assert!(terminal.tabs[0].term.screen().alternate_screen());
    assert!(terminal.scene().scroll.is_none());
    let saved: Saved = serde_json::from_str(&terminal.snapshot().data).unwrap();
    assert!(saved.tabs[0].output.contains("shell history"));
    assert!(!saved.tabs[0].output.contains("fullscreen"));
    assert!(
        terminal.tabs[0].term.screen().alternate_screen(),
        "snapshotting must not mutate the live screen"
    );
    output(&mut terminal, b"\x1b[?1049l");
    assert!(
        terminal.tabs[0]
            .term
            .screen()
            .contents()
            .contains("shell history")
    );
}

/// The selection adapter preserves word/line copy and SGR mouse reporting for applications.
#[test]
fn selection_copy_and_mouse_reporting() {
    let mut terminal = app();
    output(&mut terminal, b"hello world\r\nsecond line");
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 8. + terminal.cw,
        y: 9.,
        button: 0,
        clicks: 2,
        shift: false,
    });
    terminal.event(Event::Command {
        id: "copy".into(),
        cwd: None,
        text: None,
        arguments: None,
    });
    assert!(CALLS.with(|calls| {
        calls
            .borrow()
            .iter()
            .any(|r| matches!(r, api::Operation::Editor { operation: api::EditorOperation::WriteClipboard {text}, .. } if text == "hello"))
    }));
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 9.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    terminal.event(Event::Pointer {
        kind: "move".into(),
        x: 8. + terminal.cw * 4.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    terminal.event(Event::Pointer {
        kind: "up".into(),
        x: 8. + terminal.cw * 4.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert_eq!(
        terminal.tabs[0].term.selection_text().as_deref(),
        Some("hello")
    );
    output(&mut terminal, b"\x1b[?1000h\x1b[?1006h");
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 9.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    terminal.event(Event::Pointer {
        kind: "up".into(),
        x: 9.,
        y: 9.,
        button: 0,
        clicks: 1,
        shift: false,
    });
    assert_eq!(writes(), b"\x1b[<0;1;1M\x1b[<0;1;1m");
}

/// Resizing informs the host PTY; bounded history scrolls and snapshots obey the quota.
#[test]
fn resizing_and_history_limits() {
    let mut terminal = app();
    terminal.event(Event::Resize {
        width: 1000.,
        height: 600.,
        cell_width: 8.,
        cell_height: 20.,
    });
    assert_eq!(terminal.tabs[0].term.screen().size(), (29, 122));
    assert!(CALLS.with(|calls| calls.borrow().iter().any(|r| matches!(
        r,
        api::Operation::Process {
            operation: process::Operation::Resize {
                rows: 29,
                columns: 122,
                ..
            }
        }
    ))));
    let bytes = (0..100)
        .map(|i| format!("output {i}\r\n"))
        .collect::<String>();
    output(&mut terminal, bytes.as_bytes());
    terminal.tabs[0].term.set_history_limit(10);
    terminal.tabs[0].term.scroll(1000);
    assert_eq!(terminal.tabs[0].term.screen().scrollback(), 10);
    assert!(terminal.tabs[0].term.snapshot(100).len() <= 100);
    terminal.tabs[0].term.set_scrollback(0);
    let before = terminal.tabs[0].term.screen().contents();
    terminal.event(Event::Command {
        id: "clear".into(),
        cwd: None,
        text: None,
        arguments: None,
    });
    assert_eq!(terminal.tabs[0].term.history(), 0);
    assert_eq!(terminal.tabs[0].term.screen().contents(), before);
}

/// Pixel-size changes within one cell extent must not repeatedly notify the Shell.
#[test]
fn dragging_within_one_cell_extent_sends_only_one_pty_resize() {
    let mut terminal = app();
    CALLS.with(|calls| calls.borrow_mut().clear());
    for height in [600., 601., 602., 603.] {
        terminal.event(Event::Resize {
            width: 1000.,
            height,
            cell_width: 8.,
            cell_height: 20.,
        });
    }
    let count = CALLS.with(|calls| {
        calls
            .borrow()
            .iter()
            .filter(|request| {
                matches!(
                    request,
                    api::Operation::Process {
                        operation: process::Operation::Resize { .. }
                    }
                )
            })
            .count()
    });
    assert_eq!(count, 1, "one grid size needs one PTY resize notification");
}

/// Window resizing must not overwrite the user's current sidebar width.
#[test]
fn window_resize_preserves_selected_tab_width() {
    let mut terminal = app();
    // A user-adjusted width must survive the same window resize sequence as the default.
    terminal.tab_width = 260.;
    let chosen_width = terminal.tab_width;
    for width in [1000., 240., 160., 100., 800.] {
        terminal.event(Event::Resize {
            width,
            height: 420.,
            cell_width: 8.,
            cell_height: 20.,
        });
        let tabs = terminal.sessions();
        assert_eq!(tabs.width, chosen_width, "window width {width}");
        assert_eq!(tabs.min_width, MIN_TAB_WIDTH);
        assert_eq!(tabs.max_width, MAX_TAB_WIDTH);
    }
}

/// Grid reflow alone must not duplicate a prompt while a typed command is pending.
#[test]
fn rapid_reflow_keeps_one_unsubmitted_prompt() {
    let mut terminal = app();
    output(
        &mut terminal,
        b"PS C:\\project>Write-Output 'HELLO_RESIZE_TEST'",
    );
    let baseline: Saved = serde_json::from_str(&terminal.snapshot().data).unwrap();
    assert_eq!(baseline.tabs[0].output.matches("PS C:").count(), 1);
    for width in [300., 1100., 300., 1100.] {
        terminal.event(Event::Resize {
            width,
            height: 420.,
            cell_width: 8.,
            cell_height: 20.,
        });
    }
    let saved: Saved = serde_json::from_str(&terminal.snapshot().data).unwrap();
    assert_eq!(saved.tabs[0].output.matches("PS C:").count(), 1);
}

/// Repeated restore keeps a one-line prompt and its caret without accumulating generated rows.
#[test]
fn restoring_prompt_does_not_append_text_or_move_caret() {
    let mut terminal = app();
    output(&mut terminal, b"PS C:\\project> ");
    let contents = terminal.tabs[0].term.screen().contents();
    let cursor = terminal.tabs[0].term.screen().cursor_position();
    for _ in 0..3 {
        let snapshot = terminal.snapshot();
        let saved: Saved = serde_json::from_str(&snapshot.data).unwrap();
        assert!(!saved.tabs[0].output.ends_with("\r\n"));
        assert!(!saved.tabs[0].output.contains("restored session; new shell"));
        terminal = Terminal::prepare(terminal.env.clone(), Some(snapshot)).unwrap();
        assert_eq!(terminal.tabs[0].term.screen().contents(), contents);
        assert_eq!(terminal.tabs[0].term.screen().cursor_position(), cursor);
        assert_eq!(terminal.tabs[0].term.history(), 0);
        assert!(terminal.scene().scroll.is_none());
    }
}

/// ConPTY's first query starts the new shell at the line's beginning, keeping saved cells intact.
#[test]
fn restored_conpty_bootstrap_preserves_frame_and_avoids_prompt_gap() {
    let mut terminal = app();
    terminal.event(Event::Resize {
        width: 1600.,
        height: 380.,
        cell_width: 8.4,
        cell_height: 21.,
    });
    output(&mut terminal, b"PS C:\\project> \r\nPS C:\\project> ");
    let caret = terminal.tabs[0].term.screen().cursor_position();
    let mut restored = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    restored.activate();
    restored.event(Event::Resize {
        width: 1600.,
        height: 310.,
        cell_width: 8.4,
        cell_height: 21.,
    });
    let contents = restored.tabs[0].term.screen().contents();
    assert_eq!(restored.tabs[0].term.screen().cursor_position(), caret);
    CALLS.with(|calls| calls.borrow_mut().clear());
    // Readers can split the four-byte inherited-cursor query across arbitrary output chunks.
    for part in [b"\x1b".as_slice(), b"[6".as_slice()] {
        output(&mut restored, part);
        assert!(writes().is_empty());
    }
    output(&mut restored, b"n");
    assert_eq!(writes(), b"\x1b[2;1R");
    assert_eq!(restored.tabs[0].term.screen().contents(), contents);
    assert_eq!(restored.tabs[0].term.screen().cursor_position(), caret);
    // When the viewport is unchanged ConPTY can print directly, without a separate CUP.
    output(&mut restored, b"PS C:\\project> ");
    assert_eq!(restored.tabs[0].term.screen().contents(), contents);
    assert_eq!(restored.tabs[0].term.screen().cursor_position(), caret);
    CALLS.with(|calls| calls.borrow_mut().clear());
    // Later application queries still receive the real saved caret, never the bootstrap column.
    output(&mut restored, b"\x1b[6n");
    assert_eq!(
        writes(),
        format!("\x1b[{};{}R", caret.0 + 1, caret.1 + 1).as_bytes()
    );
}

/// Restored command output stays adjacent to its prompt across both grid resize and ConPTY startup.
#[test]
fn restored_command_transcript_keeps_spacing_after_height_changes() {
    let mut terminal = app();
    terminal.event(Event::Resize {
        width: 1600.,
        height: 310.,
        cell_width: 8.4,
        cell_height: 21.,
    });
    output(
        &mut terminal,
        b"PS C:\\project> node -v\r\nv24.19.0\r\nPS C:\\project> ",
    );
    let contents = terminal.tabs[0].term.screen().contents();
    let caret = terminal.tabs[0].term.screen().cursor_position();
    let mut restored = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    restored.activate();
    CALLS.with(|calls| calls.borrow_mut().clear());
    output(&mut restored, b"\x1b[6n");
    // Reporting the old prompt's final column lets ConPTY place the new shell one row lower;
    // its first height redraw then erases the saved prompt and reveals that empty row.
    assert_eq!(writes(), b"\x1b[3;1R");
    output(&mut restored, b"PS C:\\project> ");
    for height in [500., 200., 380., 140., 620., 310.] {
        restored.event(Event::Resize {
            width: 1600.,
            height,
            cell_width: 8.4,
            cell_height: 21.,
        });
        // Added blank viewport rows are harmless; an interior blank before the prompt is not.
        assert_eq!(
            restored.tabs[0].term.screen().contents().trim_end(),
            contents.trim_end()
        );
        assert_eq!(restored.tabs[0].term.screen().cursor_position(), caret);
    }
}

/// A fresh process must not reuse a row containing pending input, task output or custom prompts.
#[test]
fn restored_conpty_keeps_nonempty_command_and_task_output() {
    for bytes in [
        b"PS C:\\project> echo pending".as_slice(),
        b"task output without trailing newline".as_slice(),
        b"custom> ".as_slice(),
    ] {
        let mut terminal = app();
        output(&mut terminal, bytes);
        let mut restored =
            Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
        restored.activate();
        let caret = restored.tabs[0].term.screen().cursor_position();
        let contents = restored.tabs[0].term.screen().contents();
        CALLS.with(|calls| calls.borrow_mut().clear());
        output(&mut restored, b"\x1b[6n");
        assert_eq!(
            writes(),
            format!("\x1b[{};{}R", caret.0 + 1, caret.1 + 1).as_bytes()
        );
        assert_eq!(restored.tabs[0].term.screen().contents(), contents);
        // Native PowerShell starts a new prompt after the existing partial row, keeping its text.
        output(&mut restored, b"\r\nPS C:\\project> ");
        assert_eq!(
            restored.tabs[0].term.screen().contents().lines().next(),
            contents.lines().next()
        );
    }
}

/// Repair only the previous startup bug's trailing duplicate default PowerShell prompts.
#[test]
fn legacy_restore_repairs_prompt_gap_but_keeps_command_output_blank_lines() {
    let mut terminal = app();
    output(
        &mut terminal,
        b"PS C:\\project> \r\n\r\n\r\nPS C:\\project> ",
    );
    let mut old: serde_json::Value = serde_json::from_str(&terminal.snapshot().data).unwrap();
    old["recovery_version"] = 0.into();
    let restored = Terminal::prepare(
        terminal.env.clone(),
        Some(Snapshot {
            schema: 1,
            data: serde_json::to_string(&old).unwrap(),
        }),
    )
    .unwrap();
    assert_eq!(restored.tabs[0].term.screen().cursor_position().0, 1);
    assert_eq!(
        restored.tabs[0]
            .term
            .screen()
            .contents()
            .lines()
            .take(2)
            .filter(|line| line.starts_with("PS "))
            .count(),
        2
    );
    // A fresh-format snapshot retains intentional blank lines, including user-designed prompts.
    let untouched = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    assert_eq!(untouched.tabs[0].term.screen().cursor_position().0, 3);
    for output_bytes in [
        b"PS C:\\project> command\r\n\r\n\r\nPS C:\\project> ".as_slice(),
        b"text\r\n\r\nPS C:\\project> ".as_slice(),
    ] {
        let mut terminal = app();
        output(&mut terminal, output_bytes);
        let contents = terminal.tabs[0].term.screen().contents();
        let mut saved: serde_json::Value = serde_json::from_str(&terminal.snapshot().data).unwrap();
        saved["recovery_version"] = 0.into();
        let restored = Terminal::prepare(
            terminal.env.clone(),
            Some(Snapshot {
                schema: 1,
                data: serde_json::to_string(&saved).unwrap(),
            }),
        )
        .unwrap();
        assert_eq!(restored.tabs[0].term.screen().contents(), contents);
    }
}

/// Snapshot restoration preserves dimensions, blank rows, colors, scroll position and soft wraps.
#[test]
fn restoring_grid_preserves_history_styles_and_reflow() {
    let mut terminal = app();
    terminal.event(Event::Resize {
        width: 340.,
        height: 120.,
        cell_width: 8.,
        cell_height: 20.,
    });
    output(&mut terminal, b"first line\r\nsecond line\r\n");
    output(
        &mut terminal,
        "\x1b[31m长行中文 e\u{301} abcdefghijklmnopqrstuvwxyz\x1b[0m\r\n".as_bytes(),
    );
    output(
        &mut terminal,
        b"\x1b[44mtrailing  \x1b[0m\r\n\r\nPS> \x1b[2;3H",
    );
    terminal.tabs[0].term.set_scrollback(2);
    let restored = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    let before = terminal.tabs[0].term.screen();
    let after = restored.tabs[0].term.screen();
    assert_eq!(before.size(), after.size());
    assert_eq!(before.cursor_position(), after.cursor_position());
    assert_eq!(before.scrollback(), after.scrollback());
    let history = terminal.tabs[0].term.history();
    assert!(history > 0);
    assert_eq!(restored.tabs[0].term.history(), history);
    for line in -(history as i32)..before.size().0 as i32 {
        assert_eq!(before.row_wrapped(line), after.row_wrapped(line));
        for col in 0..before.size().1 {
            let before = before.cell_line(line, col).unwrap();
            let after = after.cell_line(line, col).unwrap();
            assert_eq!(
                before.contents(),
                after.contents(),
                "line {line}, col {col}"
            );
            assert_eq!(before.fgcolor(), after.fgcolor());
            assert_eq!(before.bgcolor(), after.bgcolor());
            assert_eq!(before.is_wide_continuation(), after.is_wide_continuation());
        }
    }
    let mut restored = restored;
    for tab in [&mut terminal.tabs[0], &mut restored.tabs[0]] {
        tab.term.resize(5, 30);
    }
    assert_eq!(
        terminal.tabs[0].term.snapshot(1_000_000),
        restored.tabs[0].term.snapshot(1_000_000)
    );
}

/// The old package's generated separator and writer newline are migrated out of schema 1 data.
#[test]
fn legacy_restoration_removes_generated_separator() {
    let terminal = app();
    let mut saved: Saved = serde_json::from_str(&terminal.snapshot().data).unwrap();
    saved.tabs[0].display = None;
    saved.tabs[0].output = "\x1b[0m\x1b[0mPS> \x1b[0m\r\n\x1b[0m\r\n\x1b[0m\x1b[0m--- restored session; new shell ---\x1b[0m\r\n".into();
    let restored = Terminal::prepare(
        terminal.env.clone(),
        Some(Snapshot {
            schema: 1,
            data: serde_json::to_string(&saved).unwrap(),
        }),
    )
    .unwrap();
    assert_eq!(restored.tabs[0].term.screen().contents().trim_end(), "PS>");
    assert_eq!(restored.tabs[0].term.screen().cursor_position(), (0, 4));
    assert!(
        !restored
            .snapshot()
            .data
            .contains("restored session; new shell")
    );
}

/// Clipboard keys never send Ctrl+C/Ctrl+V bytes to the shell or copy an empty selection.
#[test]
fn control_copy_paste_and_shift_aliases_use_clipboard() {
    let mut terminal = app();
    output(&mut terminal, b"hello world");
    CALLS.with(|calls| calls.borrow_mut().clear());
    terminal.event(Event::Key {
        key: "c".into(),
        ctrl: true,
        alt: false,
        shift: false,
    });
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    terminal.tabs[0].term.select(0, 0, 2);
    for shift in [false, true] {
        terminal.event(Event::Key {
            key: "c".into(),
            ctrl: true,
            alt: false,
            shift,
        });
        terminal.event(Event::Key {
            key: "v".into(),
            ctrl: true,
            alt: false,
            shift,
        });
    }
    CALLS.with(|calls| {
        let calls = calls.borrow();
        assert_eq!(
            calls
                .iter()
                .filter(|r| matches!(r, api::Operation::Editor { operation: api::EditorOperation::WriteClipboard {text}, .. } if text == "hello"))
                .count(),
            2
        );
        assert_eq!(
            calls
                .iter()
                .filter(|r| matches!(r, api::Operation::Editor { operation: api::EditorOperation::ReadClipboard, .. }))
                .count(),
            2
        );
    });
    assert!(writes().is_empty());
    terminal.command("interrupt", None, None);
    assert_eq!(
        writes(),
        vec![3],
        "the explicit interrupt action still reaches the process"
    );
}

/// A context menu stays local to the output, preserves selection and rejects disabled actions.
#[test]
fn output_context_menu_copies_only_selected_text_and_pastes() {
    let mut terminal = app();
    output(&mut terminal, b"hello world");
    CALLS.with(|calls| calls.borrow_mut().clear());
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 60.,
        y: 40.,
        button: 2,
        clicks: 1,
        shift: false,
    });
    let menu = terminal.document().menu.unwrap();
    assert_eq!((menu.x, menu.y), (60., 40.));
    assert_eq!(
        menu.items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>(),
        ["复制", "粘贴", "清空缓冲区"]
    );
    assert!(menu.items[0].disabled);
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: menu.id.clone(),
        action: ui::Action::Select("copy".into()),
    }));
    assert!(terminal.menu.is_some());
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: menu.id.clone(),
        action: ui::Action::Dismiss,
    }));
    terminal.tabs[0].term.select(0, 0, 2);
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 60.,
        y: 40.,
        button: 2,
        clicks: 1,
        shift: false,
    });
    assert_eq!(terminal.selected_text().as_deref(), Some("hello"));
    assert!(!terminal.document().menu.unwrap().items[0].disabled);
    terminal.document().validate().unwrap();
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: menu.id.clone(),
        action: ui::Action::Select("copy".into()),
    }));
    assert!(terminal.menu.is_none());
    assert!(CALLS.with(|calls| {
        calls
            .borrow()
            .iter()
            .any(|r| matches!(r, api::Operation::Editor { operation: api::EditorOperation::WriteClipboard {text}, .. } if text == "hello"))
    }));
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 60.,
        y: 40.,
        button: 2,
        clicks: 1,
        shift: false,
    });
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: menu.id,
        action: ui::Action::Select("paste".into()),
    }));
    assert!(CALLS.with(|calls| {
        calls.borrow().iter().any(|r| {
            matches!(
                r,
                api::Operation::Editor {
                    operation: api::EditorOperation::ReadClipboard,
                    ..
                }
            )
        })
    }));
    assert!(writes().is_empty());
}

/// Clearing the buffer erases visible and old lines without affecting other tabs or the shell.
#[test]
fn output_context_menu_clears_entire_active_buffer() {
    let mut terminal = app();
    output(&mut terminal, b"other session");
    terminal.add(0, "C:/second".into());
    let bytes = (0..40).map(|i| format!("line {i}\r\n")).collect::<String>();
    output(&mut terminal, bytes.as_bytes());
    output(&mut terminal, b"\x1b[?2004h\x1b[?1049halt output");
    let handle = terminal.tabs[1].handle.clone();
    terminal.tabs[1].term.select(0, 0, 2);
    CALLS.with(|calls| calls.borrow_mut().clear());
    terminal.event(Event::Pointer {
        kind: "down".into(),
        x: 60.,
        y: 40.,
        button: 2,
        clicks: 1,
        shift: false,
    });
    terminal.event(Event::Ui(ui::UiEvent {
        revision: 0,
        node: "terminal-output-menu".into(),
        action: ui::Action::Select("clear-buffer".into()),
    }));
    assert_eq!(terminal.tabs[1].handle, handle);
    assert!(terminal.tabs[1].term.screen().contents().trim().is_empty());
    assert_eq!(terminal.tabs[1].term.screen().cursor_position(), (0, 0));
    assert!(terminal.tabs[1].term.screen().bracketed_paste());
    assert!(terminal.tabs[1].term.selected_range().is_none());
    assert!(terminal.tabs[1].term.snapshot(1_000_000).is_empty());
    output(&mut terminal, b"\x1b[?1049l");
    assert_eq!(terminal.tabs[1].term.history(), 0);
    assert!(terminal.tabs[1].term.screen().contents().trim().is_empty());
    assert!(
        terminal.tabs[0]
            .term
            .screen()
            .contents()
            .contains("other session")
    );
    assert!(terminal.scene().scroll.is_none());
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
}

/// Exercise the host wire payload instead of calling a private creation helper directly.
fn invoke(terminal: &mut Terminal, id: &str, arguments: serde_json::Value) {
    let event = Event::Command {
        id: id.into(),
        cwd: None,
        text: None,
        arguments: Some(arguments),
    };
    terminal.event(event);
}

/// Duplicate labels remain independent sessions, including after closing and restoring old data.
#[test]
fn default_names_never_increment_and_legacy_names_are_preserved() {
    let mut terminal = app();
    terminal.add(0, "C:/second".into());
    terminal.add(0, "C:/third".into());
    assert!(terminal.tabs.iter().all(|tab| tab.name == "powershell"));
    assert_eq!(
        terminal
            .tabs
            .iter()
            .map(|tab| tab.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    terminal.close(1);
    let mut saved: Saved = serde_json::from_str(&terminal.snapshot().data).unwrap();
    saved.tabs[0].name = "powershell42".into();
    saved.counts.insert("powershell".into(), 42);
    let mut restored = Terminal::prepare(
        terminal.env.clone(),
        Some(Snapshot {
            schema: 1,
            data: serde_json::to_string(&saved).unwrap(),
        }),
    )
    .unwrap();
    restored.activate();
    restored.add(0, "C:/new".into());
    assert_eq!(restored.tabs[0].name, "powershell42");
    assert_eq!(restored.tabs.last().unwrap().name, "powershell");
    restored.document().validate().unwrap();
}

/// Host-provided names and profiles survive persistence while an empty name uses the tool.
#[test]
fn host_can_create_named_terminal_in_a_selected_directory() {
    let mut terminal = app();
    invoke(
        &mut terminal,
        "terminal.new",
        serde_json::json!({
            "name": "  构建输出  ", "cwd": "C:/build", "profile": 1,
        }),
    );
    let tab = &terminal.tabs[terminal.active];
    assert_eq!(tab.name, "构建输出");
    assert_eq!(tab.cwd, "C:/build");
    assert_eq!(tab.profile.program, "cmd.exe");
    assert!(
        CALLS.with(|calls| calls.borrow().iter().any(|call| matches!(call,
            api::Operation::Process { operation: process::Operation::Execute { program, cwd, .. } } if program == "cmd.exe" && cwd.as_deref() == Some("C:/build")
        )))
    );
    let restored = Terminal::prepare(terminal.env.clone(), Some(terminal.snapshot())).unwrap();
    assert_eq!(restored.tabs[restored.active].name, "构建输出");
    invoke(
        &mut terminal,
        "terminal.new",
        serde_json::json!({ "name": "\n\t\u{0000}" }),
    );
    assert_eq!(terminal.tabs[terminal.active].name, "powershell");
    invoke(
        &mut terminal,
        "terminal.new",
        serde_json::json!({ "name": "终".repeat(100) }),
    );
    assert_eq!(terminal.tabs[terminal.active].name.chars().count(), 80);
}

/// Save callbacks must retain each concurrent task's command, label, working directory and profile.
#[test]
fn host_run_parameters_survive_asynchronous_save_callbacks() {
    let mut terminal = app();
    CALLS.with(|calls| calls.borrow_mut().clear());
    for (name, cwd, command, profile) in [
        ("运行 A", "C:/a", "echo alpha", 0),
        ("运行 B", "C:/b", "echo beta", 1),
    ] {
        invoke(
            &mut terminal,
            "terminal.run",
            serde_json::json!({
                "name": name, "cwd": cwd, "command": command, "profile": profile,
            }),
        );
    }
    assert_eq!(terminal.tabs.len(), 1);
    assert!(writes().is_empty());
    assert_eq!(terminal.pending_editor.len(), 2);
    assert_eq!(terminal.pending_editor.len(), 2);
    complete_save(&mut terminal);
    assert_eq!(terminal.tabs[terminal.active].name, "运行 A");
    assert_eq!(terminal.tabs[terminal.active].cwd, "C:/a");
    assert_eq!(writes(), b"echo alpha\r");
    CALLS.with(|calls| calls.borrow_mut().clear());
    complete_save(&mut terminal);
    assert_eq!(terminal.tabs[terminal.active].name, "运行 B");
    assert_eq!(terminal.tabs[terminal.active].cwd, "C:/b");
    assert_eq!(terminal.tabs[terminal.active].profile.program, "cmd.exe");
    assert_eq!(writes(), b"echo beta\r");
    assert!(terminal.pending_editor.is_empty());
}

/// Rejected task creation must never send a host's command to the existing interactive tab.
#[test]
fn invalid_or_disabled_host_requests_do_not_run_in_an_existing_session() {
    let mut terminal = app();
    let handle = terminal.tabs[0].handle.clone();
    CALLS.with(|calls| calls.borrow_mut().clear());
    invoke(
        &mut terminal,
        "terminal.new",
        serde_json::json!({ "name": 42 }),
    );
    assert!(terminal.error.is_some());
    assert_eq!(terminal.tabs.len(), 1);
    assert!(CALLS.with(|calls| calls.borrow().is_empty()));
    for arguments in [
        serde_json::json!({ "name": "运行", "command": "echo unsafe" }),
        serde_json::json!({ "name": "无效 Shell", "profile": 99, "command": "echo unsafe" }),
    ] {
        terminal.settings.enabled = arguments.get("profile").is_some();
        invoke(&mut terminal, "terminal.run", arguments);
        complete_save(&mut terminal);
        assert_eq!(terminal.tabs.len(), 1);
        assert_eq!(terminal.tabs[0].handle, handle);
        assert!(writes().is_empty());
    }
    terminal.settings.enabled = true;
    invoke(
        &mut terminal,
        "terminal.new",
        serde_json::json!({ "name": "已修正" }),
    );
    assert!(terminal.error.is_none());
    assert_eq!(terminal.tabs[terminal.active].name, "已修正");
}

/// Older hosts omit structured arguments; their toolbar and keyboard creation still work.
#[test]
fn old_host_command_without_arguments_remains_compatible() {
    let mut terminal = app();
    invoke(&mut terminal, "terminal.new", serde_json::Value::Null);
    assert_eq!(terminal.tabs.len(), 2);
    assert_eq!(terminal.tabs[1].name, "powershell");
}
/// Drive version-aware read and save completions; admission itself must not run the command.
fn complete_save(terminal: &mut Terminal) {
    let document = api::DocumentVersion {
        id: "file".into(),
        path: "src/main.rs".into(),
        revision: 3,
    };
    let Some(id) = terminal
        .pending_editor
        .iter()
        .find_map(|(id, p)| matches!(p, commands::PendingEditor::BeforeSave(_)).then_some(*id))
    else {
        return;
    };
    terminal.editor_completion(
        request_handle(id),
        api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Selection {
                document: document.clone(),
                text: String::new(),
            }),
        },
    );
    let id = terminal
        .pending_editor
        .iter()
        .find_map(|(id, p)| matches!(p, commands::PendingEditor::AfterSave(_)).then_some(*id))
        .unwrap();
    terminal.editor_completion(
        request_handle(id),
        api::RequestUpdate::Completed {
            result: Ok(api::EditorValue::Saved { document }),
        },
    );
}
