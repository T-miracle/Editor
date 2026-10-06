//! Shared SVG parsing and raster allocation guards for native plugin images and vectors.

use plugin_runtime::plugin_protocol::api::{ErrorCode, Failure};
use resvg::{tiny_skia::Transform, usvg};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, OnceLock},
};

const NODES: u32 = 10_000;
const DEPTH: usize = 128;

/// System fonts are discovered once on a worker; image SVGs retain visible text like vector previews.
pub(super) fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut fonts = usvg::fontdb::Database::new();
            fonts.load_system_fonts();
            Arc::new(fonts)
        })
        .clone()
}

/// Reject entities, excessive XML and exponential definition reuse before usvg builds derived paths.
pub(super) fn parse(source: &str, options: &usvg::Options) -> Result<usvg::Tree, Failure> {
    if source.len() > 512 * 1024 {
        return Err(limited());
    }
    let xml = usvg::roxmltree::Document::parse_with_options(
        source,
        usvg::roxmltree::ParsingOptions {
            allow_dtd: false,
            nodes_limit: NODES,
            ..Default::default()
        },
    )
    .map_err(|_| Failure::new(ErrorCode::OperationFailed, "Invalid or excessive SVG XML"))?;
    let mut characters = 0;
    for node in xml.descendants() {
        if node.ancestors().take(DEPTH + 1).count() > DEPTH {
            return Err(limited());
        }
        if node.is_text() {
            characters += node.text().unwrap_or_default().chars().count();
            if characters > NODES as usize {
                return Err(limited());
            }
        }
        if node.is_element() {
            let (attribute, maximum) = match node.tag_name().name() {
                "feTurbulence" => ("numOctaves", 8.),
                "feConvolveMatrix" => ("order", 15.),
                _ => continue,
            };
            if let Some(value) = node.attribute(attribute) {
                // Reject dangerous loop/kernel parameters before upstream integer conversion or expansion.
                for number in svgtypes::NumberListParser::from(value) {
                    let number = number.map_err(|_| {
                        Failure::new(ErrorCode::OperationFailed, "Invalid SVG filter parameter")
                    })?;
                    if !number.is_finite() || number > maximum {
                        return Err(limited());
                    }
                }
            }
        }
    }
    // Match usvg's first definition and namespace precedence; a different target bypasses cost checks.
    let mut ids = BTreeMap::new();
    for node in xml.descendants() {
        if let Some(id) = node.attribute("id") {
            ids.entry(id).or_insert(node);
        }
    }
    let mut costs = BTreeMap::new();
    reference_cost(xml.root(), &ids, &mut costs, &mut BTreeSet::new(), 0)?;
    paint_reuse(&xml, &costs)?;
    usvg::Tree::from_xmltree(&xml, options)
        .map_err(|_| Failure::new(ErrorCode::OperationFailed, "Invalid SVG image"))
}

/// Paint servers are cloned for object bounding boxes; inherited properties and CSS hide their users.
/// Bound all potentially used definitions conservatively before conversion, without executing CSS.
fn paint_reuse(xml: &usvg::roxmltree::Document, costs: &BTreeMap<u32, u32>) -> Result<(), Failure> {
    let definition = |node: usvg::roxmltree::Node| {
        node.is_element()
            && matches!(
                node.tag_name().name(),
                "pattern"
                    | "linearGradient"
                    | "radialGradient"
                    | "filter"
                    | "mask"
                    | "clipPath"
                    | "marker"
            )
    };
    let has_url = |node: usvg::roxmltree::Node| {
        node.attributes()
            .any(|attribute| attribute.value().contains("url("))
            || (node.is_element()
                && node.tag_name().name() == "style"
                && node.text().is_some_and(|text| text.contains("url(")))
    };
    if !xml.descendants().any(has_url) {
        return Ok(());
    }
    let definitions = xml
        .descendants()
        .filter(|node| definition(*node))
        .map(|node| u64::from(costs[&node.id().get()]))
        .sum::<u64>();
    let mut users = xml
        .root_element()
        .children()
        .filter(|node| {
            !node.is_element()
                || !matches!(
                    node.tag_name().name(),
                    "defs" | "style" | "metadata" | "title" | "desc"
                )
        })
        .map(|node| u64::from(costs[&node.id().get()]))
        .sum::<u64>();
    let nested = xml.descendants().any(|node| {
        (has_url(node)
            && (node.ancestors().any(definition)
                || node.descendants().any(definition)
                || (node.is_element() && node.tag_name().name() == "style")))
            || (definition(node)
                && (node.attribute("href").is_some()
                    || node
                        .attribute(("http://www.w3.org/1999/xlink", "href"))
                        .is_some()))
    });
    // Definitions can themselves receive shared/inherited paint, so include those potential users too.
    if nested {
        users = users.saturating_add(definitions);
    }
    if definitions.saturating_mul(users) > u64::from(NODES) {
        return Err(limited());
    }
    Ok(())
}

/// Memoized expanded costs count each use independently, without actually cloning its definition.
fn reference_cost<'a, 'input>(
    node: usvg::roxmltree::Node<'a, 'input>,
    ids: &BTreeMap<&'a str, usvg::roxmltree::Node<'a, 'input>>,
    memo: &mut BTreeMap<u32, u32>,
    visiting: &mut BTreeSet<u32>,
    depth: usize,
) -> Result<u32, Failure> {
    let key = node.id().get();
    if let Some(cost) = memo.get(&key) {
        return Ok(*cost);
    }
    if depth > DEPTH || !visiting.insert(key) {
        return Err(limited());
    }
    // A single path or text node may be much more expensive than one empty group when reused.
    let attributes = node
        .attributes()
        .map(|attribute| attribute.value().len())
        .sum::<usize>();
    let text = if node.is_text() {
        node.text().unwrap_or_default().chars().count()
    } else {
        0
    };
    let mut cost = 1_u32
        .saturating_add(attributes.div_ceil(16).min(NODES as usize + 1) as u32)
        .saturating_add(text.min(NODES as usize + 1) as u32);
    if cost > NODES {
        return Err(limited());
    }
    for child in node.children() {
        cost = cost.saturating_add(reference_cost(child, ids, memo, visiting, depth + 1)?);
        if cost > NODES {
            return Err(limited());
        }
    }
    if node.is_element() && matches!(node.tag_name().name(), "use" | "feImage") {
        let href = node
            .attribute(("http://www.w3.org/1999/xlink", "href"))
            .or_else(|| node.attribute("href"));
        if let Some(target) = href
            .and_then(|href| svgtypes::IRI::from_str(href).ok().map(|iri| iri.0))
            .and_then(|id| ids.get(id))
        {
            cost = cost.saturating_add(reference_cost(*target, ids, memo, visiting, depth + 1)?);
            if cost > NODES {
                return Err(limited());
            }
        }
    }
    visiting.remove(&key);
    memo.insert(key, cost);
    Ok(cost)
}

/// Charge output and possible temporary surfaces before resvg enters filters, patterns or masks.
/// Summing even sequential surfaces is deliberately conservative; rejection affects only one image.
pub(super) fn render_budget(
    tree: &usvg::Tree,
    transform: Transform,
    width: u32,
    height: u32,
    remaining: u64,
) -> Result<(), Failure> {
    let mut budget = Budget {
        remaining: remaining.min(64 * 1024 * 1024),
        nodes: 0,
        work: 16 * 1024 * 1024,
    };
    budget.surface(width as f32, height as f32, 1)?;
    budget.group(tree.root(), transform, 0)
}

/// Pixel accounting is independent of root dimensions because effects can allocate larger surfaces.
struct Budget {
    remaining: u64,
    nodes: u32,
    /// Shared sample/iteration allowance bounds effects that consume CPU without large output buffers.
    work: u64,
}

impl Budget {
    fn surface(&mut self, width: f32, height: f32, copies: u64) -> Result<(), Failure> {
        if !width.is_finite() || !height.is_finite() || width > 4096. || height > 4096. {
            return Err(limited());
        }
        let bytes = (width.ceil().max(1.) as u64)
            .saturating_mul(height.ceil().max(1.) as u64)
            .saturating_mul(4)
            .saturating_mul(copies);
        self.remaining = self.remaining.checked_sub(bytes).ok_or_else(limited)?;
        Ok(())
    }

    fn group(
        &mut self,
        group: &usvg::Group,
        parent: Transform,
        depth: usize,
    ) -> Result<(), Failure> {
        if depth > DEPTH {
            return Err(limited());
        }
        let transform = parent.pre_concat(group.transform());
        let layer = group
            .layer_bounding_box()
            .transform(transform)
            .ok_or_else(limited)?;
        let (width, height) = (layer.width() + 4., layer.height() + 4.);
        if group.should_isolate() {
            self.surface(width, height, 1)?;
        }
        for filter in group.filters() {
            let rect = filter.rect().transform(transform).ok_or_else(limited)?;
            // Filter implementations keep previous results and allocate scratch buffers for primitives.
            self.surface(
                rect.width(),
                rect.height(),
                (filter.primitives().len() as u64)
                    .saturating_mul(8)
                    .saturating_add(4),
            )?;
            for primitive in filter.primitives() {
                self.effect_work(primitive.kind(), transform, rect.width(), rect.height())?;
                let rect = primitive.rect().transform(transform).ok_or_else(limited)?;
                self.surface(rect.width(), rect.height(), 8)?;
                if let usvg::filter::Kind::Image(image) = primitive.kind() {
                    self.group(image.root(), transform, depth + 1)?;
                }
            }
        }
        if let Some(mask) = group.mask() {
            self.mask(mask, transform, width, height, depth + 1)?;
        }
        if let Some(clip) = group.clip_path() {
            self.clip(clip, transform, width, height, depth + 1)?;
        }
        for node in group.children() {
            self.nodes += 1;
            if self.nodes > NODES {
                return Err(limited());
            }
            match node {
                usvg::Node::Group(group) => self.group(group, transform, depth + 1)?,
                usvg::Node::Text(text) => self.group(text.flattened(), transform, depth + 1)?,
                usvg::Node::Path(path) => {
                    if let Some(fill) = path.fill() {
                        self.paint(fill.paint(), transform, depth + 1)?;
                    }
                    if let Some(stroke) = path.stroke() {
                        self.paint(stroke.paint(), transform, depth + 1)?;
                    }
                }
                usvg::Node::Image(_) => {}
            }
        }
        Ok(())
    }

    /// Match upstream parameter-driven loops before any primitive can overflow or monopolize the actor.
    fn effect_work(
        &mut self,
        kind: &usvg::filter::Kind,
        transform: Transform,
        width: f32,
        height: f32,
    ) -> Result<(), Failure> {
        use usvg::filter::Kind;
        let (sx, sy) = transform.get_scale();
        let samples = match kind {
            Kind::Turbulence(effect) => {
                if effect.num_octaves() > 8 {
                    return Err(limited());
                }
                u64::from(effect.num_octaves()).saturating_mul(4)
            }
            Kind::ConvolveMatrix(effect) => {
                let matrix = effect.matrix();
                if matrix.columns() > 15 || matrix.rows() > 15 {
                    return Err(limited());
                }
                u64::from(matrix.columns()).saturating_mul(u64::from(matrix.rows()))
            }
            Kind::Morphology(effect) => {
                let x = physical(effect.radius_x().get(), sx, 64.)?;
                let y = physical(effect.radius_y().get(), sy, 64.)?;
                (x.ceil() as u64 * 2)
                    .min(width.ceil() as u64)
                    .saturating_mul((y.ceil() as u64 * 2).min(height.ceil() as u64))
            }
            Kind::Merge(effect) => effect.inputs().len() as u64,
            Kind::GaussianBlur(effect) => {
                physical(effect.std_dev_x().get(), sx, 64.)?;
                physical(effect.std_dev_y().get(), sy, 64.)?;
                20
            }
            Kind::DropShadow(effect) => {
                physical(effect.std_dev_x().get(), sx, 64.)?;
                physical(effect.std_dev_y().get(), sy, 64.)?;
                physical(effect.dx(), sx, 4096.)?;
                physical(effect.dy(), sy, 4096.)?;
                24
            }
            Kind::DisplacementMap(effect) => {
                physical(effect.scale(), sx.max(sy), 128.)?;
                4
            }
            Kind::Offset(effect) => {
                physical(effect.dx(), sx, 4096.)?;
                physical(effect.dy(), sy, 4096.)?;
                1
            }
            _ => 1,
        };
        let work = (width.ceil() as u64)
            .saturating_mul(height.ceil() as u64)
            .saturating_mul(samples.max(1));
        self.work = self.work.checked_sub(work).ok_or_else(limited)?;
        Ok(())
    }

    fn paint(
        &mut self,
        paint: &usvg::Paint,
        transform: Transform,
        depth: usize,
    ) -> Result<(), Failure> {
        if let usvg::Paint::Pattern(pattern) = paint {
            let (sx, sy) = transform.pre_concat(pattern.transform()).get_scale();
            self.surface(pattern.rect().width() * sx, pattern.rect().height() * sy, 1)?;
            self.group(pattern.root(), Transform::from_scale(sx, sy), depth + 1)?;
        }
        Ok(())
    }

    fn mask(
        &mut self,
        mask: &usvg::Mask,
        transform: Transform,
        width: f32,
        height: f32,
        depth: usize,
    ) -> Result<(), Failure> {
        if depth > DEPTH {
            return Err(limited());
        }
        self.surface(width, height, 2)?;
        self.group(mask.root(), transform, depth + 1)?;
        if let Some(next) = mask.mask() {
            self.mask(next, transform, width, height, depth + 1)?;
        }
        Ok(())
    }

    fn clip(
        &mut self,
        clip: &usvg::ClipPath,
        transform: Transform,
        width: f32,
        height: f32,
        depth: usize,
    ) -> Result<(), Failure> {
        if depth > DEPTH {
            return Err(limited());
        }
        self.surface(width, height, 2)?;
        self.group(
            clip.root(),
            transform.pre_concat(clip.transform()),
            depth + 1,
        )?;
        if let Some(next) = clip.clip_path() {
            self.clip(next, transform, width, height, depth + 1)?;
        }
        Ok(())
    }
}

/// Bound scaled numeric parameters before renderer integer casts, radius arithmetic or index offsets.
fn physical(value: f32, scale: f32, maximum: f64) -> Result<f32, Failure> {
    let value = f64::from(value) * f64::from(scale);
    if !value.is_finite() || value.abs() > maximum {
        return Err(limited());
    }
    Ok(value.abs() as f32)
}

/// A typed quota failure lets the native caption explain why only this resource was rejected.
fn limited() -> Failure {
    Failure::new(
        ErrorCode::LimitExceeded,
        "SVG parsing or temporary raster quota exceeded",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build only a parsed tree; a dangerous filter must never be executed to test its work limit.
    fn source(effect: &str, size: u32) -> String {
        format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{size}\" height=\"{size}\"><defs><filter id=\"f\" filterUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{size}\" height=\"{size}\">{effect}</filter></defs><rect width=\"{size}\" height=\"{size}\" filter=\"url(#f)\"/></svg>"
        )
    }

    /// Tiny SVGs can still ask for billions of noise iterations or enormous convolution kernels.
    #[test]
    fn excessive_effect_parameters_fail_before_svg_conversion() {
        let turbulence = source("<feTurbulence numOctaves=\"1000000000\"/>", 1);
        let convolution = source(
            &format!(
                "<feConvolveMatrix order=\"64\" kernelMatrix=\"{}\"/>",
                "1 ".repeat(64 * 64)
            ),
            1,
        );
        let denied = [&turbulence, &convolution]
            .map(|source| parse(source, &usvg::Options::default()).is_err());
        assert_eq!(
            denied, [true; 2],
            "parameter limits apply before a costly renderer is entered"
        );
    }

    /// Work scales with pixels times kernel size, rather than just the small number of primitives.
    #[test]
    fn bounded_effects_still_charge_area_times_work() {
        let convolution = source(
            &format!(
                "<feConvolveMatrix order=\"15\" kernelMatrix=\"{}\"/>",
                "1 ".repeat(225)
            ),
            512,
        );
        let tree = parse(&convolution, &usvg::Options::default()).unwrap();
        assert!(render_budget(&tree, Transform::identity(), 512, 512, 64 * 1024 * 1024).is_err());
        let morphology = source("<feMorphology radius=\"1000000000\"/>", 1);
        let tree = parse(&morphology, &usvg::Options::default()).unwrap();
        assert!(render_budget(&tree, Transform::identity(), 1, 1, 64 * 1024 * 1024).is_err());
    }

    /// Ordinary small effects remain supported within the same parsing and raster limits.
    #[test]
    fn normal_small_filter_is_supported() {
        let tree = parse(
            &source("<feTurbulence numOctaves=\"3\"/>", 16),
            &usvg::Options::default(),
        )
        .unwrap();
        assert!(render_budget(&tree, Transform::identity(), 16, 16, 64 * 1024 * 1024).is_ok());
    }

    /// Huge blur sigma can overflow upstream radius arithmetic even for a tiny explicit filter region.
    #[test]
    fn excessive_blur_and_shadow_sigma_are_rejected_before_rendering() {
        let denied = [
            "<feGaussianBlur stdDeviation=\"1e20\"/>",
            "<feDropShadow stdDeviation=\"1e20\"/>",
        ]
        .map(|effect| {
            let tree = parse(&source(effect, 1), &usvg::Options::default()).unwrap();
            render_budget(&tree, Transform::identity(), 1, 1, 64 * 1024 * 1024).is_err()
        });
        assert_eq!(denied, [true; 2]);
    }

    /// Reusing a long path is expensive even when each definition contains just one XML node.
    #[test]
    fn expanded_geometry_payload_is_bounded_before_conversion() {
        let data = format!("M0 0 {}", "L1 0 L0 1 ".repeat(100));
        let source = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"><defs><path id=\"p\" d=\"{data}\"/></defs>{}</svg>",
            "<use href=\"#p\"/>".repeat(400)
        );
        assert!(
            parse(&source, &usvg::Options::default()).is_err(),
            "expanded payload, not just expanded node count, must be charged"
        );
    }

    /// Object-bounding-box paint conversion clones its definition per user, including inherited CSS.
    #[test]
    fn repeated_paint_definitions_are_bounded_before_conversion() {
        let definition = format!(
            "<defs><pattern id=\"p\" width=\"0.5\" height=\"0.5\">{}</pattern></defs>",
            "<rect width=\"1\" height=\"1\" fill=\"red\"/>".repeat(200)
        );
        let shapes = (1..=80)
            .map(|index| format!("<rect class=\"paint\" width=\"{index}\" height=\"1\"/>"))
            .collect::<String>();
        let direct = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\">{definition}<g fill=\"url(#p)\">{shapes}</g></svg>"
        );
        let styled = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\">{definition}<style>.paint {{ fill: url(#p); }}</style>{shapes}</svg>"
        );
        let denied =
            [&direct, &styled].map(|source| parse(source, &usvg::Options::default()).is_err());
        assert_eq!(
            denied, [true; 2],
            "inherited/CSS paint use must be charged before converted groups are cloned"
        );
    }
}
