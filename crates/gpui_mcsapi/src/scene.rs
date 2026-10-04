use egui::epaint::{
    ClippedShape, CornerRadius, Mesh, PathShape, RectShape, Stroke, StrokeKind, Vertex,
};
use egui::{Color32, Pos2, Rect, Shape, TextureId, pos2, vec2};
use gpui::{
    AtlasTextureId, AtlasTile, BackgroundPaint, Bounds, ColorSpace, ContentMask, Corners, Hsla,
    PaddedBool32, PrimitiveBatch, ScaledPixels, Scene, TransformationMatrix,
};

/// Where an atlas texture lives on the egui side, as the atlas reports it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextureSlot {
    pub id: TextureId,
    pub size: [f32; 2],
}

/// Turns a finished GPUI scene into egui shapes in window points.
///
/// GPUI's renderers draw each primitive kind with its own shader. egui has rounded rectangles,
/// blurred rectangles, meshes and polylines, so each primitive becomes the closest of those, in
/// GPUI's paint order. What egui cannot express is approximated, and noted where it happens.
pub(crate) fn scene_to_shapes(
    scene: &Scene,
    scale_factor: f32,
    texture: &dyn Fn(AtlasTextureId) -> Option<TextureSlot>,
) -> Vec<ClippedShape> {
    let mut converter = Converter {
        scale_factor,
        shapes: Vec::new(),
    };
    for batch in scene.batches() {
        match batch {
            PrimitiveBatch::Shadows(range) => {
                for shadow in &scene.shadows[range] {
                    if shadow.inset != 0 {
                        // egui has no inset shadow. Leaving it out keeps the element's own fill
                        // and border correct, which matters more than the inner glow.
                        continue;
                    }
                    converter.push(
                        &shadow.content_mask,
                        RectShape::filled(
                            converter.rect(&shadow.bounds),
                            converter.corners(&shadow.corner_radii),
                            color(shadow.color),
                        )
                        .with_blur_width(shadow.blur_radius.0 * 2.0 / scale_factor)
                        .into(),
                    );
                }
            }
            PrimitiveBatch::Quads(range) => {
                for quad in &scene.quads[range] {
                    converter.quad(quad);
                }
            }
            PrimitiveBatch::Paths(range) => {
                for path in &scene.paths[range] {
                    converter.path(path);
                }
            }
            PrimitiveBatch::Underlines(range) => {
                for underline in &scene.underlines[range] {
                    converter.underline(underline);
                }
            }
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                let Some(slot) = texture(texture_id) else {
                    log::warn!("monochrome sprites refer to unknown atlas texture {texture_id:?}");
                    continue;
                };
                for sprite in &scene.monochrome_sprites[range] {
                    converter.sprite(
                        &sprite.content_mask,
                        &sprite.bounds,
                        &sprite.tile,
                        &sprite.transformation,
                        color(sprite.color),
                        slot,
                    );
                }
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                // The atlas stores subpixel glyphs as plain coverage, see `atlas.rs`.
                let Some(slot) = texture(texture_id) else {
                    log::warn!("subpixel sprites refer to unknown atlas texture {texture_id:?}");
                    continue;
                };
                for sprite in &scene.subpixel_sprites[range] {
                    converter.sprite(
                        &sprite.content_mask,
                        &sprite.bounds,
                        &sprite.tile,
                        &sprite.transformation,
                        color(sprite.color),
                        slot,
                    );
                }
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                let Some(slot) = texture(texture_id) else {
                    log::warn!("polychrome sprites refer to unknown atlas texture {texture_id:?}");
                    continue;
                };
                for sprite in &scene.polychrome_sprites[range] {
                    // `grayscale` needs a shader, so such images stay in color. The tint keeps
                    // their opacity.
                    let alpha = (sprite.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let tint = Color32::from_white_alpha(alpha);
                    let rect = converter.rect(&sprite.bounds);
                    let uv = uv_rect(&sprite.tile, slot);
                    converter.push(
                        &sprite.content_mask,
                        RectShape::filled(rect, converter.corners(&sprite.corner_radii), tint)
                            .with_texture(slot.id, uv)
                            .into(),
                    );
                }
            }
            // Surfaces are CoreVideo buffers, which exist only on macOS.
            PrimitiveBatch::Surfaces(_) => {}
        }
    }
    converter.shapes
}

struct Converter {
    scale_factor: f32,
    shapes: Vec<ClippedShape>,
}

impl Converter {
    fn point(&self, x: ScaledPixels, y: ScaledPixels) -> Pos2 {
        pos2(x.0 / self.scale_factor, y.0 / self.scale_factor)
    }

    fn rect(&self, bounds: &Bounds<ScaledPixels>) -> Rect {
        Rect::from_min_size(
            self.point(bounds.origin.x, bounds.origin.y),
            vec2(
                bounds.size.width.0 / self.scale_factor,
                bounds.size.height.0 / self.scale_factor,
            ),
        )
    }

    fn corners(&self, corners: &Corners<ScaledPixels>) -> CornerRadius {
        let radius =
            |value: ScaledPixels| (value.0 / self.scale_factor).round().clamp(0.0, 255.0) as u8;
        CornerRadius {
            nw: radius(corners.top_left),
            ne: radius(corners.top_right),
            sw: radius(corners.bottom_left),
            se: radius(corners.bottom_right),
        }
    }

    fn push(&mut self, mask: &ContentMask<ScaledPixels>, shape: Shape) {
        let clip_rect = self.rect(&mask.bounds);
        if clip_rect.is_positive() {
            self.shapes.push(ClippedShape { clip_rect, shape });
        }
    }

    fn quad(&mut self, quad: &gpui::Quad) {
        let rect = self.rect(&quad.bounds);
        let corner_radius = self.corners(&quad.corner_radii);
        let fill = match quad.background.paint() {
            BackgroundPaint::Solid(fill) => color(fill),
            BackgroundPaint::LinearGradient {
                angle,
                stops,
                color_space,
            } => {
                self.gradient(&quad.content_mask, rect, angle, stops, color_space);
                Color32::TRANSPARENT
            }
            // Stripes and checks would need a texture per size. Half the color reads as the
            // same "disabled" or "transparent" hint those patterns are used for.
            BackgroundPaint::PatternSlash(fill) => color(fill).gamma_multiply(0.5),
            BackgroundPaint::Checkerboard { color: fill, .. } => color(fill).gamma_multiply(0.5),
        };
        if fill.a() > 0 {
            self.push(
                &quad.content_mask,
                RectShape::filled(rect, corner_radius, fill).into(),
            );
        }

        let border = color(quad.border_color);
        if border.a() == 0 {
            return;
        }
        let widths = [
            quad.border_widths.top.0,
            quad.border_widths.right.0,
            quad.border_widths.bottom.0,
            quad.border_widths.left.0,
        ]
        .map(|width| width / self.scale_factor);
        if widths.iter().all(|width| *width <= 0.0) {
            return;
        }
        if widths
            .iter()
            .all(|width| (width - widths[0]).abs() < f32::EPSILON)
        {
            self.push(
                &quad.content_mask,
                RectShape::stroke(
                    rect,
                    corner_radius,
                    Stroke::new(widths[0], border),
                    StrokeKind::Inside,
                )
                .into(),
            );
            return;
        }
        // egui strokes have one width, and GPUI often borders a single side (a tab's bottom
        // edge, a panel's divider), so each side becomes its own strip. Rounded corners on
        // such borders are drawn square.
        let [top, right, bottom, left] = widths;
        let strips = [
            Rect::from_min_max(rect.min, pos2(rect.max.x, rect.min.y + top)),
            Rect::from_min_max(pos2(rect.max.x - right, rect.min.y), rect.max),
            Rect::from_min_max(pos2(rect.min.x, rect.max.y - bottom), rect.max),
            Rect::from_min_max(rect.min, pos2(rect.min.x + left, rect.max.y)),
        ];
        for strip in strips {
            if strip.is_positive() {
                self.push(
                    &quad.content_mask,
                    RectShape::filled(strip, CornerRadius::ZERO, border).into(),
                );
            }
        }
    }

    fn gradient(
        &mut self,
        mask: &ContentMask<ScaledPixels>,
        rect: Rect,
        angle: f32,
        stops: [gpui::LinearColorStop; 2],
        _color_space: ColorSpace,
    ) {
        // A four-vertex mesh interpolates linearly in sRGB. Oklab gradients come out slightly
        // different in the middle; the ends are exact.
        let radians = angle.to_radians();
        let direction = vec2(radians.sin(), -radians.cos());
        let center = rect.center();
        let half_length =
            (rect.width() * direction.x.abs() + rect.height() * direction.y.abs()) / 2.0;
        let start = color(stops[0].color);
        let end = color(stops[1].color);
        let (from, to) = (
            stops[0].percentage,
            stops[1].percentage.max(stops[0].percentage + 1e-3),
        );
        let color_at = |corner: Pos2| {
            let along = if half_length > 0.0 {
                ((corner - center).dot(direction) / half_length + 1.0) / 2.0
            } else {
                0.0
            };
            let t = ((along - from) / (to - from)).clamp(0.0, 1.0);
            lerp_color(start, end, t)
        };
        let mut mesh = Mesh::default();
        for corner in [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ] {
            mesh.vertices.push(Vertex {
                pos: corner,
                uv: egui::epaint::WHITE_UV,
                color: color_at(corner),
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        self.push(mask, Shape::mesh(mesh));
    }

    fn path(&mut self, path: &gpui::Path<ScaledPixels>) {
        let fill = match path.color.paint() {
            BackgroundPaint::Solid(fill) => color(fill),
            BackgroundPaint::LinearGradient { stops, .. } => {
                lerp_color(color(stops[0].color), color(stops[1].color), 0.5)
            }
            BackgroundPaint::PatternSlash(fill)
            | BackgroundPaint::Checkerboard { color: fill, .. } => color(fill).gamma_multiply(0.5),
        };
        let mut mesh = Mesh::default();
        for triangle in path.vertices.chunks_exact(3) {
            let points = [0, 1, 2].map(|index| {
                let position = triangle[index].xy_position;
                self.point(position.x, position.y)
            });
            let st = [0, 1, 2].map(|index| triangle[index].st_position);
            // PathBuilder tessellates with lyon and marks solid triangles with st = (0, 1).
            // A triangle built by `Path::curve_to` carries the Loop-Blinn coordinates
            // (0, 0), (0.5, 0), (1, 1) instead: the fill is the part between its chord and the
            // quadratic curve through its corners, which a fan from the start point covers.
            let is_curve = st.iter().any(|st| (st.x, st.y) != (0.0, 1.0));
            if is_curve {
                const SEGMENTS: usize = 8;
                let curve = |t: f32| {
                    let inverse = 1.0 - t;
                    pos2(
                        inverse * inverse * points[0].x
                            + 2.0 * inverse * t * points[1].x
                            + t * t * points[2].x,
                        inverse * inverse * points[0].y
                            + 2.0 * inverse * t * points[1].y
                            + t * t * points[2].y,
                    )
                };
                for segment in 1..SEGMENTS - 1 {
                    let first = curve(segment as f32 / (SEGMENTS - 1) as f32);
                    let second = curve((segment + 1) as f32 / (SEGMENTS - 1) as f32);
                    push_triangle(&mut mesh, [points[0], first, second], fill);
                }
            } else {
                push_triangle(&mut mesh, points, fill);
            }
        }
        if !mesh.is_empty() {
            self.push(&path.content_mask, Shape::mesh(mesh));
        }
    }

    fn underline(&mut self, underline: &gpui::Underline) {
        let rect = self.rect(&underline.bounds);
        let thickness = underline.thickness.0 / self.scale_factor;
        let stroke_color = color(underline.color);
        if underline.wavy == PaddedBool32::from(true) {
            let amplitude = (rect.height() - thickness).max(thickness) / 2.0;
            let wavelength = (amplitude * 4.0).max(2.0);
            let middle = rect.center().y;
            let steps = ((rect.width() / wavelength) * 8.0).ceil().max(2.0) as usize;
            let points = (0..=steps)
                .map(|step| {
                    let x = rect.min.x + rect.width() * step as f32 / steps as f32;
                    let phase = (x - rect.min.x) / wavelength * std::f32::consts::TAU;
                    pos2(x, middle + amplitude * phase.sin())
                })
                .collect();
            self.push(
                &underline.content_mask,
                PathShape::line(points, Stroke::new(thickness, stroke_color)).into(),
            );
        } else {
            let line = Rect::from_min_size(rect.min, vec2(rect.width(), thickness));
            self.push(
                &underline.content_mask,
                RectShape::filled(line, CornerRadius::ZERO, stroke_color).into(),
            );
        }
    }

    fn sprite(
        &mut self,
        mask: &ContentMask<ScaledPixels>,
        bounds: &Bounds<ScaledPixels>,
        tile: &AtlasTile,
        transformation: &TransformationMatrix,
        tint: Color32,
        slot: TextureSlot,
    ) {
        let rect = self.rect(bounds);
        let uv = uv_rect(tile, slot);
        let corners = [
            rect.left_top(),
            rect.right_top(),
            rect.right_bottom(),
            rect.left_bottom(),
        ];
        let uvs = [
            uv.left_top(),
            uv.right_top(),
            uv.right_bottom(),
            uv.left_bottom(),
        ];
        let mut mesh = Mesh::with_texture(slot.id);
        for (corner, uv) in corners.into_iter().zip(uvs) {
            mesh.vertices.push(Vertex {
                pos: self.transform(corner, transformation),
                uv,
                color: tint,
            });
        }
        mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
        self.push(mask, Shape::mesh(mesh));
    }

    /// Applies a sprite's transformation, which GPUI expresses in scaled pixels.
    fn transform(&self, point: Pos2, transformation: &TransformationMatrix) -> Pos2 {
        let [[a, b], [c, d]] = transformation.rotation_scale;
        let [tx, ty] = transformation.translation;
        let x = point.x * self.scale_factor;
        let y = point.y * self.scale_factor;
        pos2(
            (a * x + b * y + tx) / self.scale_factor,
            (c * x + d * y + ty) / self.scale_factor,
        )
    }
}

fn push_triangle(mesh: &mut Mesh, points: [Pos2; 3], fill: Color32) {
    let base = mesh.vertices.len() as u32;
    for pos in points {
        mesh.vertices.push(Vertex {
            pos,
            uv: egui::epaint::WHITE_UV,
            color: fill,
        });
    }
    mesh.indices.extend_from_slice(&[base, base + 1, base + 2]);
}

fn uv_rect(tile: &AtlasTile, slot: TextureSlot) -> Rect {
    let origin = tile.bounds.origin;
    let size = tile.bounds.size;
    Rect::from_min_size(
        pos2(
            origin.x.0 as f32 / slot.size[0],
            origin.y.0 as f32 / slot.size[1],
        ),
        vec2(
            size.width.0 as f32 / slot.size[0],
            size.height.0 as f32 / slot.size[1],
        ),
    )
}

pub(crate) fn color(color: Hsla) -> Color32 {
    let rgba = color.to_rgb();
    let channel = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgba_unmultiplied(
        channel(rgba.r),
        channel(rgba.g),
        channel(rgba.b),
        channel(rgba.a),
    )
}

fn lerp_color(start: Color32, end: Color32, t: f32) -> Color32 {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(
        mix(start.r(), end.r()),
        mix(start.g(), end.g()),
        mix(start.b(), end.b()),
        mix(start.a(), end.a()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{Edges, Quad, ScaledPixels, point, size};

    fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<ScaledPixels> {
        Bounds {
            origin: point(ScaledPixels(x), ScaledPixels(y)),
            size: size(ScaledPixels(width), ScaledPixels(height)),
        }
    }

    fn quad(border_widths: Edges<ScaledPixels>) -> Quad {
        Quad {
            bounds: bounds(20.0, 40.0, 200.0, 100.0),
            content_mask: ContentMask {
                bounds: bounds(0.0, 0.0, 1000.0, 1000.0),
            },
            background: gpui::solid_background(gpui::red()),
            border_color: gpui::blue(),
            border_widths,
            ..Default::default()
        }
    }

    fn rects(shapes: &[ClippedShape]) -> Vec<RectShape> {
        shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                Shape::Rect(rect) => Some(rect.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn quads_land_in_points() {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(Edges::all(ScaledPixels(4.0))));
        scene.finish();
        let shapes = rects(&scene_to_shapes(&scene, 2.0, &|_| None));
        assert_eq!(shapes.len(), 2, "a fill and one uniform stroke");
        assert_eq!(
            shapes[0].rect,
            Rect::from_min_size(pos2(10.0, 20.0), vec2(100.0, 50.0))
        );
        assert_eq!(shapes[0].fill, Color32::RED);
        assert_eq!(shapes[1].stroke.width, 2.0);
    }

    #[test]
    fn one_sided_border_is_a_strip() {
        let mut scene = Scene::default();
        scene.insert_primitive(quad(Edges {
            bottom: ScaledPixels(2.0),
            ..Default::default()
        }));
        scene.finish();
        let shapes = rects(&scene_to_shapes(&scene, 1.0, &|_| None));
        assert_eq!(shapes.len(), 2, "a fill and the bottom strip");
        assert_eq!(
            shapes[1].rect,
            Rect::from_min_max(pos2(20.0, 138.0), pos2(220.0, 140.0))
        );
        assert_eq!(shapes[1].fill, Color32::BLUE);
    }
}
