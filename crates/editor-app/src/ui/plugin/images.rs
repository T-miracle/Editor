//! Rasterize clipped plugin vectors off the UI thread and reuse unchanged scene images.

use gpui_kit::RenderImage;
use plugin_runtime::plugin_protocol::{Paint, Rect, Scene};
use resvg::{tiny_skia, usvg};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// The image bounds describe the visible raster; the SVG may extend beyond them after zooming.
#[derive(Clone)]
pub(crate) struct VectorImage {
    pub rect: Rect,
    pub image: Arc<RenderImage>,
}

/// Every image slot corresponds to one drawing operation in the same immutable scene.
pub(crate) type SceneImages = BTreeMap<String, Arc<Vec<Option<VectorImage>>>>;

/// Font discovery and parsing are cached while the worker owns the currently published scenes.
#[derive(Default)]
pub(crate) struct VectorRenderer {
    /// Derived canvas surfaces retain their identities while the owning native document stays unchanged.
    nested: BTreeMap<String, (Arc<Scene>, Arc<Scene>)>,
    scenes: BTreeMap<String, (Arc<Scene>, Arc<Vec<Option<VectorImage>>>)>,
    trees: BTreeMap<[u8; 32], Arc<usvg::Tree>>,
    fonts: Option<Arc<usvg::fontdb::Database>>,
}

impl VectorRenderer {
    /// Repaint only changed scene vectors, retaining native image IDs across idle worker polls.
    pub fn prepare(&mut self, scenes: &BTreeMap<String, Arc<Scene>>) -> SceneImages {
        let mut surfaces = scenes.clone();
        let mut nested_keys = BTreeSet::new();
        for (key, scene) in scenes {
            if let Some(document) = &scene.ui {
                let mut visit = |node: &plugin_runtime::plugin_protocol::ui::Node| {
                    if let plugin_runtime::plugin_protocol::ui::Kind::Canvas(drawing) = &node.kind {
                        let key = format!("{key}/canvas/{}", node.id);
                        nested_keys.insert(key.clone());
                        if self
                            .nested
                            .get(&key)
                            .is_none_or(|(owner, _)| !Arc::ptr_eq(owner, scene))
                        {
                            self.nested.insert(
                                key.clone(),
                                (
                                    scene.clone(),
                                    Arc::new(Scene {
                                        paint: drawing.paint.clone(),
                                        ..Default::default()
                                    }),
                                ),
                            );
                        }
                        surfaces.insert(key.clone(), self.nested[&key].1.clone());
                    }
                };
                document.root.visit(&mut visit);
                if let Some(dialog) = &document.dialog {
                    dialog.content.visit(&mut visit);
                }
            }
        }
        self.nested.retain(|key, _| nested_keys.contains(key));
        let scenes = &surfaces;
        self.scenes.retain(|key, _| scenes.contains_key(key));
        let mut used = BTreeSet::new();
        for (key, scene) in scenes {
            if !scene
                .paint
                .iter()
                .any(|operation| matches!(operation, Paint::Svg { .. }))
            {
                // Ordinary text/terminal scenes need no parallel array of empty image slots.
                self.scenes.remove(key);
                continue;
            }
            for operation in &scene.paint {
                if let Paint::Svg { source, .. } = operation {
                    used.insert(<[u8; 32]>::from(Sha256::digest(source.as_bytes())));
                }
            }
            if self
                .scenes
                .get(key)
                .is_some_and(|(old, _)| Arc::ptr_eq(old, scene))
            {
                continue;
            }
            let mut images = Vec::with_capacity(scene.paint.len());
            for (index, operation) in scene.paint.iter().enumerate() {
                let image = if let Paint::Svg { source, rect, clip } = operation {
                    // A focus or theme update can change labels without changing the vector itself.
                    let cached =
                        self.scenes
                            .get(key)
                            .and_then(|(old, images)| match old.paint.get(index) {
                                Some(Paint::Svg {
                                    source: old_source,
                                    rect: old_rect,
                                    clip: old_clip,
                                }) if source == old_source
                                    && rect == old_rect
                                    && clip == old_clip =>
                                {
                                    images.get(index).cloned().flatten()
                                }
                                _ => None,
                            });
                    cached.or_else(|| self.rasterize(source, *rect, *clip))
                } else {
                    None
                };
                images.push(image);
            }
            self.scenes
                .insert(key.clone(), (scene.clone(), Arc::new(images)));
        }
        // Removed documents and plugin packages must not retain their decoded resources forever.
        self.trees.retain(|digest, _| used.contains(digest));
        self.scenes
            .iter()
            .map(|(key, (_, images))| (key.clone(), images.clone()))
            .collect()
    }

    /// Preserve SVG alpha, block ambient file reads, and allocate only the visible clipped area.
    fn rasterize(&mut self, source: &str, rect: Rect, clip: Rect) -> Option<VectorImage> {
        let target = intersection(rect, clip)?;
        let digest = <[u8; 32]>::from(Sha256::digest(source.as_bytes()));
        let tree = if let Some(tree) = self.trees.get(&digest) {
            tree.clone()
        } else {
            let fonts = self
                .fonts
                .get_or_insert_with(|| {
                    let mut fonts = usvg::fontdb::Database::new();
                    fonts.load_system_fonts();
                    Arc::new(fonts)
                })
                .clone();
            let options = usvg::Options {
                fontdb: fonts,
                image_href_resolver: usvg::ImageHrefResolver {
                    // Embedded data images remain supported; arbitrary filesystem/URL reads do not.
                    resolve_string: Box::new(|_, _| None),
                    ..Default::default()
                },
                ..Default::default()
            };
            let tree = Arc::new(usvg::Tree::from_str(source, &options).ok()?);
            self.trees.insert(digest, tree.clone());
            tree
        };
        // Two pixels per logical pixel keep normal high-DPI previews sharp; cap one raster at 16 MiB.
        let density = 2_f32.min(2048. / target.w).min(2048. / target.h);
        let width = (target.w * density).ceil().clamp(1., 2048.) as u32;
        let height = (target.h * density).ceil().clamp(1., 2048.) as u32;
        let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
        let transform = tiny_skia::Transform::from_row(
            rect.w / tree.size().width() * density,
            0.,
            0.,
            rect.h / tree.size().height() * density,
            (rect.x - target.x) * density,
            (rect.y - target.y) * density,
        );
        resvg::render(&tree, transform, &mut pixmap.as_mut());
        let mut pixels = pixmap.take();
        for pixel in pixels.chunks_exact_mut(4) {
            // GPUI expects straight BGRA; tiny-skia emits premultiplied RGBA.
            let alpha = u32::from(pixel[3]);
            if alpha > 0 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
            pixel.swap(0, 2);
        }
        let buffer = image::RgbaImage::from_raw(width, height, pixels)?;
        Some(VectorImage {
            rect: target,
            image: Arc::new(RenderImage::new([image::Frame::new(buffer)])),
        })
    }
}

/// Empty intersections need neither a raster allocation nor a native image draw.
fn intersection(rect: Rect, clip: Rect) -> Option<Rect> {
    let x = rect.x.max(clip.x);
    let y = rect.y.max(clip.y);
    let w = (rect.x + rect.w).min(clip.x + clip.w) - x;
    let h = (rect.y + rect.h).min(clip.y + clip.h) - y;
    (w > 0. && h > 0.).then_some(Rect { x, y, w, h })
}
