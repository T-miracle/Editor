//! Image file viewer: editable SVG source and read-only raster images share the public file surface.

use plugin_protocol::{
    Environment, Paint, Rect, Snapshot, api,
    bindings::{Guest, export},
    ui,
};
use std::cell::RefCell;

mod display;
mod scene;

/// Zoom is the absolute scale of the SVG's intrinsic dimensions.
const MIN_SCALE: f32 = 0.01;
const MAX_SCALE: f32 = 32.;
const HEADER_HEIGHT: f32 = 32.;
/// The shared drawing protocol bounds both image extents and coordinates to one million pixels.
const MAX_EXTENT: f32 = 1_000_000.;
/// SVG assets stay inside the WASM component and are also included in the installable package.
const TOOLBAR_ICONS: [(&str, &str); 4] = [
    ("zoom-in", include_str!("../icons/zoom-in.svg")),
    ("zoom-out", include_str!("../icons/zoom-out.svg")),
    ("actual-size", include_str!("../icons/actual-size.svg")),
    ("fit", include_str!("../icons/fit-window.svg")),
];

/// Automatic sizing follows viewport/document changes until the user chooses a manual zoom.
#[derive(Clone, Copy)]
enum ViewMode {
    DefaultSize,
    FitWindow,
    Manual,
}

/// A preview owns transient view state; the editor remains the authority for document contents.
struct State {
    /// Plugin intent applies only to editable SVG, independently of raster file identity.
    display: display::Display,
    environment: Environment,
    width: f32,
    height: f32,
    /// Echoed source authority; reopening a path creates a different document identity.
    document: Option<api::DocumentVersion>,
    /// Raster previews carry file authority without manufacturing a text session or revision.
    file: Option<api::FileContext>,
    revision: u64,
    source: String,
    intrinsic: Option<(f32, f32)>,
    error: Option<String>,
    scale: f32,
    view_mode: ViewMode,
    /// A release activates only the same left-button target that received the press.
    pressed_button: Option<usize>,
    hovered_button: Option<usize>,
}

impl Default for State {
    /// Start with an empty preview, using dimensions that the first native resize will replace.
    fn default() -> Self {
        Self {
            display: Default::default(),
            environment: Environment::default(),
            width: 400.,
            height: 300.,
            document: None,
            file: None,
            revision: 0,
            source: String::new(),
            intrinsic: None,
            error: None,
            scale: 1.,
            view_mode: ViewMode::DefaultSize,
            pressed_button: None,
            hovered_button: None,
        }
    }
}

thread_local! { static STATE: RefCell<State> = RefCell::new(State::default()); }

struct ImagePreview;

impl Guest for ImagePreview {
    /// The typed lifecycle never reads the source document from disk or retains it in snapshots.
    fn dispatch(payload: String) -> Result<String, String> {
        api::guest::dispatch(&payload, |message| {
            STATE.with(|cell| {
                let mut state = cell.borrow_mut();
                match message {
                    api::Input::Prepare {
                        environment,
                        snapshot,
                        ..
                    } => {
                        if snapshot.is_some_and(|snapshot| snapshot.schema != 1) {
                            return Err(api::Failure::new(
                                api::ErrorCode::InvalidRequest,
                                "Unsupported SVG preview snapshot",
                            ));
                        }
                        *state = State {
                            environment,
                            ..Default::default()
                        };
                    }
                    api::Input::Event { panel, event } => state.event(panel.as_deref(), event)?,
                    api::Input::Snapshot => {
                        return Ok(api::Output {
                            snapshot: Some(Snapshot {
                                schema: 1,
                                data: "{}".into(),
                            }),
                            ..Default::default()
                        });
                    }
                    api::Input::Activate => {}
                }
                Ok(api::Output {
                    views: vec![state.view()],
                    ..Default::default()
                })
            })
        })
    }
}
export!(ImagePreview);
impl State {
    /// Preview is the host-managed current-document subscription; unrelated panels cannot retarget it.
    fn event(&mut self, panel: Option<&str>, event: api::Notification) -> Result<(), api::Failure> {
        match event {
            api::Notification::FilePreview { file } if panel == Some("preview") => {
                self.display.bind(file.as_ref())?;
                self.file = file;
                if self.file.as_ref().and_then(|file| file.text.as_ref()) != self.document.as_ref()
                {
                    self.document = None;
                    self.source.clear();
                    self.intrinsic = None;
                    self.error = None;
                    self.pressed_button = None;
                    self.hovered_button = None;
                }
                self.revision = self.revision.saturating_add(1);
            }
            api::Notification::Tool(event) if panel == Some("preview") => {
                if event.revision == self.revision
                    && self.file.as_ref().is_some_and(|file| {
                        event.target
                            == (ui::ToolTarget::File {
                                version: file.version.clone(),
                            })
                    })
                    && self.display.select(&event)?
                {
                    self.revision = self.revision.saturating_add(1);
                }
            }
            event @ (api::Notification::PreferenceChanged { .. }
            | api::Notification::SubscriptionFailed { .. }) => {
                // Revoked watches must reach the binding so reopening the scope can read again.
                if self.display.changed(&event)? {
                    self.revision = self.revision.saturating_add(1);
                }
            }
            api::Notification::Theme(environment) => self.environment = environment,
            api::Notification::Preview { document, text } if panel == Some("preview") => {
                self.document(document, text)
            }
            api::Notification::Ui(event)
                if panel == Some("preview") && event.node == "preview-canvas" =>
            {
                if let ui::Action::Canvas(event) = event.action {
                    self.canvas_event(event);
                }
            }
            _ => {}
        }
        Ok(())
    }
    /// Coordinates are local to this ordinary canvas; the host never interprets SVG zoom or toolbar commands.
    fn canvas_event(&mut self, event: ui::CanvasEvent) {
        match event {
            ui::CanvasEvent::Resize { width, height, .. }
                if width.is_finite() && height.is_finite() =>
            {
                self.width = width.clamp(0., 10_000.);
                self.height = height.clamp(0., 10_000.);
                match self.view_mode {
                    ViewMode::DefaultSize => self.default_size(),
                    ViewMode::FitWindow => self.fit(),
                    ViewMode::Manual => {}
                }
            }
            ui::CanvasEvent::Wheel { delta_y, x, y, .. } if self.viewport().contains(x, y) => {
                // Native wheel deltas are logical pixels; a UI-font line preserves the previous zoom step.
                let delta = delta_y / self.environment.ui_font.size_px.unwrap_or(14.).max(1.);
                if delta.is_finite() && delta != 0. {
                    self.zoom(self.scale * 1.12_f32.powf(delta.clamp(-100., 100.)));
                }
            }
            ui::CanvasEvent::Pointer {
                phase,
                x,
                y,
                button,
                ..
            } => self.pointer(phase, x, y, button),
            _ => {}
        }
    }

    /// Parse unsaved source with external image resolution disabled, keeping malformed input recoverable.
    fn document(&mut self, document: Option<api::DocumentVersion>, source: String) {
        if let Some(file) = &mut self.file {
            file.text = document.clone();
        }
        if matches!((&self.document, &document), (Some(current), Some(next)) if current.id == next.id && next.revision < current.revision)
        {
            return;
        }
        let changed_file = self.document.as_ref().map(|source| &source.id)
            != document.as_ref().map(|source| &source.id);
        if changed_file {
            self.pressed_button = None;
            self.hovered_button = None;
        }
        if changed_file {
            self.revision = self.revision.saturating_add(1);
        }
        self.document = document;
        self.intrinsic = None;
        self.error = None;
        self.source.clear();
        if self.document.is_none() {
            return;
        }
        if source.len() > 1024 * 1024 {
            self.error = Some("SVG 超过 1 MiB，无法预览".into());
            return;
        }
        let options = usvg::Options {
            image_href_resolver: usvg::ImageHrefResolver {
                resolve_string: Box::new(|_, _| None),
                ..Default::default()
            },
            ..Default::default()
        };
        match usvg::Tree::from_str(&source, &options) {
            Ok(tree) => {
                if tree.size().width().max(tree.size().height()) > MAX_EXTENT / MIN_SCALE {
                    self.error = Some("SVG 尺寸超出预览范围".into());
                    return;
                }
                self.intrinsic = Some((tree.size().width(), tree.size().height()));
                self.source = source;
                if changed_file {
                    self.default_size();
                } else {
                    match self.view_mode {
                        ViewMode::DefaultSize => self.default_size(),
                        ViewMode::FitWindow => self.fit(),
                        ViewMode::Manual => {}
                    }
                }
            }
            Err(error) => {
                // Keep the guest live while the user is midway through editing an SVG tag.
                self.error = Some(error.to_string());
            }
        }
    }

    /// Reserve the toolbar height independently of the image and its checkerboard layer.
    fn viewport(&self) -> Rect {
        Rect {
            x: 0.,
            y: self.toolbar_height(),
            w: self.width,
            h: (self.height - self.toolbar_height()).max(0.),
        }
    }

    /// Narrow split panes put the right-aligned percentage on a second toolbar row.
    fn toolbar_height(&self) -> f32 {
        if self.width < 220. {
            56.
        } else {
            HEADER_HEIGHT
        }
    }

    /// Shared paint/hit geometry keeps the four icon targets reachable down to a 100-pixel pane.
    fn toolbar_button_rect(&self, index: usize) -> Rect {
        let step = ((self.width - 8.) / 4.).clamp(0., 32.);
        Rect {
            x: 4. + index as f32 * step,
            y: 2.,
            w: step.min(28.),
            h: 28.,
        }
    }

    /// Keep pointer activation inside the toolbar and cancel releases outside the pressed icon.
    fn pointer(&mut self, phase: ui::PointerPhase, x: f32, y: f32, button: u8) {
        let target = (0..TOOLBAR_ICONS.len()).find(|index| {
            let rect = self.toolbar_button_rect(*index);
            rect.w > 0. && rect.contains(x, y)
        });
        self.hovered_button = target;
        match phase {
            ui::PointerPhase::Down if button == 0 => self.pressed_button = target,
            ui::PointerPhase::Up if button == 0 => {
                if let Some(index) = self
                    .pressed_button
                    .take()
                    .filter(|index| Some(*index) == target)
                {
                    self.command(TOOLBAR_ICONS[index].0);
                }
            }
            _ => {}
        }
    }

    /// Icon and protocol commands share the same centered image geometry as wheel zoom.
    fn command(&mut self, id: &str) {
        if self.intrinsic.is_none() {
            return;
        }
        match id {
            "zoom-in" => self.zoom(self.scale * 1.12),
            "zoom-out" => self.zoom(self.scale / 1.12),
            "actual-size" => {
                self.scale = 1.;
                self.view_mode = ViewMode::Manual;
                // Huge intrinsic documents still obey the shared rendering geometry bounds.
                self.scale = self.scale.min(self.max_scale());
            }
            "fit" => self.fit(),
            _ => {}
        }
    }

    /// The board and SVG share one rectangle; empty margins remain the ordinary panel background.
    fn image_rect(&self) -> Option<Rect> {
        let (width, height) = self.intrinsic?;
        let viewport = self.viewport();
        let w = width * self.scale;
        let h = height * self.scale;
        Some(Rect {
            x: viewport.x + (viewport.w - w) / 2.,
            y: viewport.y + (viewport.h - h) / 2.,
            w,
            h,
        })
    }

    /// Preserve intrinsic dimensions, shrinking only when either viewport axis is too small.
    fn default_size(&mut self) {
        self.view_mode = ViewMode::DefaultSize;
        if let Some((width, height)) = self.intrinsic {
            let viewport = self.viewport();
            self.scale = 1_f32.min(viewport.w / width).min(viewport.h / height);
        }
    }

    /// Fit the complete SVG to the current viewport, allowing small images to grow as well.
    fn fit(&mut self) {
        self.view_mode = ViewMode::FitWindow;
        if let Some((width, height)) = self.intrinsic {
            self.scale = self.fit_scale(width, height).min(self.max_scale());
        }
    }

    /// Change only scale; image_rect derives the centered position for every frame and viewport size.
    fn zoom(&mut self, scale: f32) {
        if self.intrinsic.is_none() {
            return;
        }
        self.scale = scale.clamp(self.min_scale(), self.max_scale());
        self.view_mode = ViewMode::Manual;
    }

    /// Very large intrinsic canvases reach the native geometry limit before the ordinary zoom cap.
    fn max_scale(&self) -> f32 {
        self.intrinsic
            .map(|(width, height)| {
                (MAX_EXTENT / width.max(height)).min(MAX_SCALE.max(self.fit_scale(width, height)))
            })
            .unwrap_or(MAX_SCALE)
    }

    /// Huge canvases must remain able to reach complete containment below one percent.
    fn min_scale(&self) -> f32 {
        self.intrinsic
            .map(|(width, height)| MIN_SCALE.min(self.fit_scale(width, height)))
            .unwrap_or(MIN_SCALE)
    }

    /// Leave a 24-pixel margin on each side while maintaining the intrinsic aspect ratio.
    fn fit_scale(&self, width: f32, height: f32) -> f32 {
        let viewport = self.viewport();
        ((viewport.w - 48.).max(1.) / width).min((viewport.h - 48.).max(1.) / height)
    }
}

#[cfg(test)]
mod tests;
