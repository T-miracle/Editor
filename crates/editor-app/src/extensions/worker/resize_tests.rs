//! Exercise resize bursts through the actual guest actor and the native publication boundary.
use super::*;

/// Inspect the dimensions and centered image supplied to the native canvas, not guest-private state.
fn geometry(state: &Published) -> Option<(f32, Rect)> {
    let document = state.views.get("svg/preview")?;
    let ui::Kind::Canvas(canvas) = &document.root.kind else {
        return None;
    };
    let width = canvas.paint.iter().find_map(|paint| match paint {
        Paint::Fill { rect, .. } if rect.x == 0. && rect.y == 0. => Some(rect.w),
        _ => None,
    })?;
    let image = canvas.paint.iter().find_map(|paint| match paint {
        Paint::Svg { rect, clip, .. } if clip.y > 0. => Some(*rect),
        _ => None,
    })?;
    Some((width, image))
}

/// A stopped resize drag must not replay queued intermediate toolbar/image positions afterwards.
#[test]
#[ignore = "build current Image package with scripts/build-plugins.ps1 -Packages svg first"]
fn image_resize_burst_publishes_latest_geometry_without_replaying_old_positions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("plugins");
    let package = Package::read(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/plugins/svg.zip"),
    )
    .unwrap();
    let mut manager = Manager::open(root.clone(), Environment::default()).unwrap();
    manager
        .install(&package, package.manifest.permissions.clone())
        .unwrap();
    drop(manager);
    let worker = Worker::start_background(root, Environment::default(), true);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !worker
        .state
        .lock()
        .unwrap()
        .views
        .contains_key("svg/preview")
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    let epoch = worker.state.lock().unwrap().instance_epochs["svg"];
    worker.tx.send(Work::Event("svg".into(), epoch, Some("preview".into()),
        api::Notification::Preview {
            document: Some(api::DocumentVersion { id: "resize.svg".into(), path: "resize.svg".into(), revision: 1 }),
            text: "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 200 100\"><rect width=\"200\" height=\"100\" fill=\"red\"/></svg>".into(),
        })).unwrap();
    while geometry(&worker.state.lock().unwrap()).is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    {
        // Pause native publication while a complete drag burst enters the production channel.
        // The focus event is an ordering barrier, so the actor cannot consume a partial burst.
        let state = worker.state.lock().unwrap();
        let revision = state.views["svg/preview"].revision;
        let event = |action| {
            Work::Event(
                "svg".into(),
                epoch,
                Some("preview".into()),
                api::Notification::Ui(ui::UiEvent {
                    revision,
                    node: "preview-canvas".into(),
                    action: ui::Action::Canvas(action),
                }),
            )
        };
        worker
            .tx
            .send(event(ui::CanvasEvent::Focus { focused: true }))
            .unwrap();
        for width in 601..=900 {
            worker
                .tx
                .send(event(ui::CanvasEvent::Resize {
                    width: width as f32,
                    height: 500.,
                    grid: None,
                }))
                .unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        // Release the publication lock before assertions so a failed test cannot poison the actor.
        let current = geometry(&worker.state.lock().unwrap());
        if let Some((width, image)) = current {
            assert!(
                width == 400. || width == 900.,
                "obsolete intermediate width published after drag: {width}"
            );
            if width == 900. {
                assert_eq!(image.x + image.w / 2., 450.);
                assert_eq!(image.y + image.h / 2., 266.);
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "latest resize never reached native preview"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        // A wheel between two resize bursts is a real input boundary, not replaceable geometry.
        let state = worker.state.lock().unwrap();
        let revision = state.views["svg/preview"].revision;
        let event = |action| {
            Work::Event(
                "svg".into(),
                epoch,
                Some("preview".into()),
                api::Notification::Ui(ui::UiEvent {
                    revision,
                    node: "preview-canvas".into(),
                    action: ui::Action::Canvas(action),
                }),
            )
        };
        worker
            .tx
            .send(event(ui::CanvasEvent::Focus { focused: true }))
            .unwrap();
        worker
            .tx
            .send(event(ui::CanvasEvent::Resize {
                width: 800.,
                height: 550.,
                grid: None,
            }))
            .unwrap();
        worker
            .tx
            .send(event(ui::CanvasEvent::Wheel {
                x: 400.,
                y: 200.,
                delta_x: 0.,
                delta_y: 14.,
                shift: false,
            }))
            .unwrap();
        for width in 901..=1000 {
            worker
                .tx
                .send(event(ui::CanvasEvent::Resize {
                    width: width as f32,
                    height: 550.,
                    grid: None,
                }))
                .unwrap();
        }
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let current = geometry(&worker.state.lock().unwrap());
        if let Some((1000., image)) = current {
            assert!(
                (image.w - 268.8).abs() < 0.01,
                "wheel zoom was lost across resize"
            );
            assert_eq!(image.x + image.w / 2., 500.);
            assert_eq!(image.y + image.h / 2., 291.);
            break;
        }
        assert!(Instant::now() < deadline, "resize/input boundary stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
    let (tx, rx) = futures::channel::oneshot::channel();
    worker.tx.send(Work::Shutdown(Some(tx))).unwrap();
    futures::executor::block_on(rx).unwrap();
}
