//! Draw file-authorized pixels using the sizing policy supplied by the guest.
use super::*;
use gpui_kit::component::ActiveTheme;
use gpui_kit::{
    AnyElement, Bounds, InteractiveElement, ParentElement, Styled, div, point, px, size,
};
use plugin_runtime::plugin_protocol::{api::ContentVersion, ui::ImageSizing};

impl PluginView {
    /// Native containment handles both axes; intrinsic limits prevent unrequested enlargement.
    pub(super) fn render_file_image(
        &self,
        id: &str,
        alt: &str,
        sizing: ImageSizing,
        cx: &App,
    ) -> AnyElement {
        let photo = self.photos.get(id).filter(|photo| {
            matches!(&photo.resource.source, ContentVersion::File(version) if self.document.file.as_ref() == Some(version))
        });
        if let Some(photo) = photo
            && let Ok(Some(bitmap)) = &photo.decoded
        {
            let bitmap = bitmap.clone();
            // Compute from the actual paint area on every frame, without a second layout state.
            let image = gpui_kit::canvas(
                |bounds, _, _| bounds,
                move |bounds, _, window, _| {
                    let (width, height) = contained_size(
                        bitmap.width as f32,
                        bitmap.height as f32,
                        bounds.size.width / px(1.),
                        bounds.size.height / px(1.),
                        sizing,
                    );
                    let target = Bounds::new(
                        bounds.origin
                            + point(
                                (bounds.size.width - px(width)) / 2.,
                                (bounds.size.height - px(height)) / 2.,
                            ),
                        size(px(width), px(height)),
                    );
                    let _ = window.paint_image(
                        bounds,
                        target,
                        Default::default(),
                        bitmap.image.clone(),
                        0,
                        false,
                    );
                },
            )
            .size_full();
            return div()
                .debug_selector(|| "plugin-file-image".into())
                .size_full()
                .min_w_0()
                .min_h_0()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center()
                .child(image)
                .into_any_element();
        }
        let key = match photo.map(|photo| &photo.decoded) {
            Some(Err(error)) => match error.code {
                plugin_runtime::plugin_protocol::api::ErrorCode::PermissionDenied => {
                    "preview.image_denied"
                }
                plugin_runtime::plugin_protocol::api::ErrorCode::InvalidPath => {
                    "preview.image_path"
                }
                plugin_runtime::plugin_protocol::api::ErrorCode::NotFound => {
                    "preview.image_missing"
                }
                plugin_runtime::plugin_protocol::api::ErrorCode::TimedOut => {
                    "preview.image_timeout"
                }
                plugin_runtime::plugin_protocol::api::ErrorCode::LimitExceeded => {
                    "preview.image_limit"
                }
                plugin_runtime::plugin_protocol::api::ErrorCode::UnsupportedOperation => {
                    "preview.image_unsupported"
                }
                _ => "preview.image_failed",
            },
            _ => "preview.image_loading",
        };
        let label = rust_i18n::t!(key, locale = self.environment.locale.as_str());
        div()
            .debug_selector(move || format!("plugin-file-image-status-{key}").into())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_color(cx.theme().muted_foreground)
            .child(if alt.is_empty() {
                label.to_string()
            } else {
                format!("{alt} · {label}")
            })
            .into_any_element()
    }
}

/// Containment preserves aspect ratio on both axes; only an explicit Contain policy may enlarge.
fn contained_size(
    width: f32,
    height: f32,
    available_width: f32,
    available_height: f32,
    sizing: ImageSizing,
) -> (f32, f32) {
    let scale = (available_width.max(0.) / width).min(available_height.max(0.) / height);
    let scale = if sizing == ImageSizing::OriginalContain {
        scale.min(1.)
    } else {
        scale
    };
    (width * scale, height * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boundary fixtures cover no enlargement, exact fit and containment on each independent axis.
    #[test]
    fn file_image_paint_geometry_preserves_intrinsic_ratio_and_fits_both_axes() {
        for (image, area, expected) in [
            ((100., 50.), (400., 300.), (100., 50.)),
            ((100., 50.), (100., 50.), (100., 50.)),
            ((800., 200.), (400., 300.), (400., 100.)),
            ((200., 800.), (400., 300.), (75., 300.)),
            ((800., 600.), (400., 300.), (400., 300.)),
            ((100., 50.), (40., 300.), (40., 20.)),
            ((100., 50.), (400., 300.), (100., 50.)),
        ] {
            assert_eq!(
                contained_size(
                    image.0,
                    image.1,
                    area.0,
                    area.1,
                    ImageSizing::OriginalContain
                ),
                expected
            );
        }
    }
}
