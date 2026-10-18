//! Headless visual-test harness: render the app's egui output to a `ColorImage`
//! so we can *see* what is being built. Tessellate the shapes, sample the font
//! atlas texture, and rasterize triangles into pixels.
//!
//! A deliberately minimal software rasterizer (no GPU). PNG encoding lives in
//! the test (the `image` crate is a dev-dependency).

use eframe::egui::{self, Color32, ColorImage, FullOutput, Pos2, Rect, TextureId};

/// A visible text run extracted from the rendered output (for assertions).
#[derive(Debug, Clone)]
pub struct VisRun {
    pub text: String,
    pub pos: Pos2,
    pub font: String,
    /// (text, font family) of every section in the run.
    pub sections: Vec<(String, String)>,
    pub color: Color32,
    pub rect: Rect,
}

/// Extract visible text runs (with origin, font, color) from a FullOutput.
pub fn visible_layout(out: &FullOutput) -> Vec<VisRun> {
    let mut runs = Vec::new();
    for clipped in &out.shapes {
        if let egui::Shape::Text(t) = &clipped.shape {
            let text = t.galley.text().to_string();
            if text.is_empty() {
                continue;
            }
            let family = |f: &egui::FontFamily| match f {
                egui::FontFamily::Name(n) => n.to_string(),
                egui::FontFamily::Proportional => "Proportional".into(),
                egui::FontFamily::Monospace => "Monospace".into(),
            };
            let (font, color) = t
                .galley
                .job
                .sections
                .first()
                .map(|s| (family(&s.format.font_id.family), s.format.color))
                .unwrap_or_default();
            let sections = t
                .galley
                .job
                .sections
                .iter()
                .map(|s| (text[s.byte_range.start.0..s.byte_range.end.0].to_string(), family(&s.format.font_id.family)))
                .collect();
            let r = t.galley.rect;
            runs.push(VisRun {
                text,
                pos: t.pos,
                font,
                sections,
                color,
                rect: Rect::from_min_size(t.pos, r.size()),
            });
        }
    }
    runs
}

fn font_atlas(out: &FullOutput) -> Option<ColorImage> {
    out.textures_delta
        .set
        .get(&TextureId::Managed(0))
        .and_then(|deltas| deltas.first())
        .and_then(|d| match &d.image {
            eframe::egui::epaint::ImageData::Color(img) => Some((**img).clone()),
        })
}

/// Rasterize a FullOutput into a `ColorImage` of the given size.
pub fn rasterize(out: &FullOutput, size: [usize; 2]) -> ColorImage {
    let atlas = font_atlas(out);
    let font_tex_size = atlas
        .as_ref()
        .map(|a| a.size)
        .unwrap_or([1, 1]);
    let mut img = ColorImage::filled(size, Color32::from_rgb(18, 18, 18));

    let mut tess = egui::epaint::Tessellator::new(
        1.0,
        egui::epaint::TessellationOptions::default(),
        font_tex_size,
        Vec::new(),
    );
    let meshes = tess.tessellate_shapes(out.shapes.clone());

    for cp in &meshes {
        if let egui::epaint::Primitive::Mesh(mesh) = &cp.primitive {
            rasterize_mesh(&mut img, mesh, cp.clip_rect, atlas.as_ref());
        }
    }
    img
}

fn sample_atlas(atlas: &ColorImage, uv: Pos2) -> Color32 {
    let w = atlas.size[0] as f32;
    let h = atlas.size[1] as f32;
    let x = (uv.x * w).round().clamp(0.0, (w - 1.0).max(0.0)) as usize;
    let y = (uv.y * h).round().clamp(0.0, (h - 1.0).max(0.0)) as usize;
    atlas.pixels[y * atlas.size[0] + x]
}

fn rasterize_mesh(
    img: &mut ColorImage,
    mesh: &egui::epaint::Mesh,
    clip: Rect,
    atlas: Option<&ColorImage>,
) {
    let (w, h) = (img.size[0], img.size[1]);
    let n = mesh.vertices.len();
    if n < 3 {
        return;
    }
    for tri in mesh.indices.chunks_exact(3) {
        let [i0, i1, i2] = [tri[0] as usize, tri[1] as usize, tri[2] as usize];
        if i0 >= n || i1 >= n || i2 >= n {
            continue;
        }
        let v = [mesh.vertices[i0], mesh.vertices[i1], mesh.vertices[i2]];
        let min_x = v.iter().map(|p| p.pos.x).fold(f32::INFINITY, f32::min);
        let max_x = v.iter().map(|p| p.pos.x).fold(f32::NEG_INFINITY, f32::max);
        let min_y = v.iter().map(|p| p.pos.y).fold(f32::INFINITY, f32::min);
        let max_y = v.iter().map(|p| p.pos.y).fold(f32::NEG_INFINITY, f32::max);
        let x0 = (min_x.floor() as i32).max(0).max(clip.min.x.floor() as i32);
        let x1 = (max_x.ceil() as i32)
            .min(w as i32 - 1)
            .min(clip.max.x.ceil() as i32 - 1);
        let y0 = (min_y.floor() as i32).max(0).max(clip.min.y.floor() as i32);
        let y1 = (max_y.ceil() as i32)
            .min(h as i32 - 1)
            .min(clip.max.y.ceil() as i32 - 1);
        if x0 > x1 || y0 > y1 {
            continue;
        }
        let (ax, ay) = (v[0].pos.x, v[0].pos.y);
        let (bx, by) = (v[1].pos.x, v[1].pos.y);
        let (cx, cy) = (v[2].pos.x, v[2].pos.y);
        let area = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
        if area.abs() < 1e-6 {
            continue;
        }
        for py in y0..=y1 {
            for px in x0..=x1 {
                let (mx, my) = (px as f32 + 0.5, py as f32 + 0.5);
                let w0 = ((bx - mx) * (cy - my) - (cx - mx) * (by - my)) / area;
                let w1 = ((cx - mx) * (ay - my) - (ax - mx) * (cy - my)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let uv = Pos2::new(
                    w0 * v[0].uv.x + w1 * v[1].uv.x + w2 * v[2].uv.x,
                    w0 * v[0].uv.y + w1 * v[1].uv.y + w2 * v[2].uv.y,
                );
                let c = [v[0].color, v[1].color, v[2].color];
                let col = Color32::from_rgba_premultiplied(
                    (w0 * c[0].r() as f32 + w1 * c[1].r() as f32 + w2 * c[2].r() as f32) as u8,
                    (w0 * c[0].g() as f32 + w1 * c[1].g() as f32 + w2 * c[2].g() as f32) as u8,
                    (w0 * c[0].b() as f32 + w1 * c[1].b() as f32 + w2 * c[2].b() as f32) as u8,
                    (w0 * c[0].a() as f32 + w1 * c[1].a() as f32 + w2 * c[2].a() as f32) as u8,
                );
                let mask = atlas
                    .map(|a| sample_atlas(a, uv))
                    .unwrap_or(Color32::WHITE);
                let fr = mask.r() as f32 * col.r() as f32 / 255.0;
                let fg = mask.g() as f32 * col.g() as f32 / 255.0;
                let fb = mask.b() as f32 * col.b() as f32 / 255.0;
                let fa = mask.a() as f32 * col.a() as f32 / 255.0;
                let idx = py as usize * w + px as usize;
                let bg = img.pixels[idx];
                let inv = 1.0 - fa / 255.0;
                img.pixels[idx] = Color32::from_rgba_premultiplied(
                    (fr + bg.r() as f32 * inv) as u8,
                    (fg + bg.g() as f32 * inv) as u8,
                    (fb + bg.b() as f32 * inv) as u8,
                    (fa + bg.a() as f32 * inv) as u8,
                );
            }
        }
    }
}
