//! Guest tests exercise the same event/snapshot interface without native OS resources.
use super::*;
thread_local! {static CALLS:RefCell<Vec<Request>>=const{RefCell::new(vec![])};}
pub(super) fn host(request: Request) -> Result<serde_json::Value, String> {
    CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        let value = match &request {
            Request::ReadData { .. } | Request::ReadWorkspace { .. } => {
                return Err("Missing fixture file".into());
            }
            Request::Spawn { .. } => serde_json::json!(calls.len() + 1),
            _ => serde_json::Value::Null,
        };
        calls.push(request);
        Ok(value)
    })
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
    let handle = app.tabs[0].handle.unwrap();
    app.event(Event::ProcessOutput {
        handle,
        bytes: b"PS C:\\project> ".to_vec(),
    });
    assert!(app.scene().scroll.is_none());
    let output = (0..70).map(|i| format!("line{i}\r\n")).collect::<String>();
    app.event(Event::ProcessOutput {
        handle,
        bytes: output.into_bytes(),
    });
    let scene = app.scene();
    let scroll = scene.scroll.unwrap();
    assert_eq!(scroll.hide_after_ms, Some(1000));
    assert_eq!(scroll.rect.x + scroll.rect.w, app.width - 180.);
    assert!(scroll.content > scroll.rect.h);
    app.event(Event::Scroll {
        id: "output".into(),
        offset: 0.,
    });
    assert!(app.tabs[0].term.screen().scrollback() > 0);
}

/// Session identity, user names, cwd and history survive replacement without replaying commands.
#[test]
fn restore_recreates_shells_but_never_replays_old_input() {
    let mut app = app();
    app.add(0, "C:/second".into());
    assert_eq!(app.tabs[0].name, "powershell");
    assert_eq!(app.tabs[1].name, "powershell2");
    let id = app.tabs[1].id;
    app.event(Event::Edit {
        id: format!("rename:{id}"),
        text: "构建任务".into(),
    });
    let handle = app.tabs[1].handle.unwrap();
    app.event(Event::ProcessOutput {
        handle,
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
            .filter(|r| matches!(r, Request::Spawn { .. }))
            .count()),
        2
    );
    assert!(!CALLS.with(|c| {
        c.borrow()
            .iter()
            .any(|r| matches!(r, Request::Write { .. }))
    }));
}

/// A selected right-side tab joins the grid; only inactive/filler edges are separated.
#[test]
fn selected_tab_has_no_left_separator() {
    let app = app();
    let right = app.width - 180.;
    let scene = app.scene();
    assert!(!scene.paint.iter().any(|p|matches!(p,Paint::Fill{rect,color}if rect.x==right&&rect.y==0.&&rect.w==1.&&*color==app.env.border)));
    assert!(scene.paint.iter().any(|p|matches!(p,Paint::Fill{rect,color}if rect.x==right&&rect.y==32.&&rect.w==1.&&*color==app.env.border)));
}

/// Cwd metadata remains intact even when ConPTY splits an escape across reads.
#[test]
fn shell_directory_metadata_can_cross_output_chunks() {
    let mut app = app();
    let handle = app.tabs[0].handle.unwrap();
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "C:/changed");
    let bytes = format!("\x1b]633;P;Cwd64={encoded}\x07").into_bytes();
    for chunk in bytes.chunks(3) {
        app.event(Event::ProcessOutput {
            handle,
            bytes: chunk.to_vec(),
        });
    }
    assert_eq!(app.tabs[0].cwd, "C:/changed");
}

/// Feed the public output event using the active host resource handle.
fn output(app: &mut Terminal, bytes: &[u8]) {
    app.event(Event::ProcessOutput {
        handle: app.tabs[app.active].handle.unwrap(),
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
                Request::Write { bytes, .. } => Some(bytes.clone()),
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
    assert_eq!(snapshot.schema, 1);
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
    });
    assert!(CALLS.with(|calls| {
        calls
            .borrow()
            .iter()
            .any(|r| matches!(r, Request::ClipboardWrite(text) if text == "hello"))
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
    assert_eq!(terminal.tabs[0].term.screen().size(), (29, 99));
    assert!(CALLS.with(|calls| calls.borrow().iter().any(|r| matches!(
        r,
        Request::Resize {
            rows: 29,
            columns: 99,
            ..
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
            .filter(|request| matches!(request, Request::Resize { .. }))
            .count()
    });
    assert_eq!(count, 1, "one grid size needs one PTY resize notification");
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
