//! Track native projection use separately from worker Arcs so GPUI textures have an explicit lifetime.

use gpui_kit::{App, Global, ImageId, RenderImage};
use std::{collections::BTreeMap, sync::Arc};

/// One count per live projection, rather than per tree node or immutable worker/test Arc.
#[derive(Default)]
struct AtlasUsers(BTreeMap<ImageId, usize>);

impl Global for AtlasUsers {}

/// Body, toolbar and dialog projections can borrow identical pixels without evicting each other.
#[derive(Default)]
pub(super) struct ImageLeases(BTreeMap<ImageId, Arc<RenderImage>>);

impl ImageLeases {
    /// Replace this projection's immutable pixel set and return images no other projection uses.
    /// The caller evicts these through App::drop_image with its currently borrowed Window.
    pub fn update(
        &mut self,
        images: impl IntoIterator<Item = Arc<RenderImage>>,
        cx: &mut App,
    ) -> Vec<Arc<RenderImage>> {
        let next = images
            .into_iter()
            .map(|image| (image.id, image))
            .collect::<BTreeMap<_, _>>();
        let users = cx.default_global::<AtlasUsers>();
        // Acquire replacements before releasing old uses, including duplicate nodes in one projection.
        for id in next.keys().filter(|id| !self.0.contains_key(id)) {
            *users.0.entry(*id).or_default() += 1;
        }
        let mut retired = Vec::new();
        for (id, image) in self.0.iter().filter(|(id, _)| !next.contains_key(id)) {
            let count = users
                .0
                .get_mut(id)
                .expect("every image lease has an atlas user");
            *count -= 1;
            if *count == 0 {
                users.0.remove(id);
                retired.push(image.clone());
            }
        }
        self.0 = next;
        retired
    }

    /// Entity release ends every use even when a hidden projection will never render again.
    pub fn clear(&mut self, cx: &mut App) -> Vec<Arc<RenderImage>> {
        self.update(std::iter::empty(), cx)
    }
}
