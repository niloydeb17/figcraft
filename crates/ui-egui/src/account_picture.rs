//! The signed-in account's picture (the browser build's server hands it over: Google's, for an
//! account that signs in with Google): decoded to a small square texture and drawn as a circle,
//! with the initials in a circle of one colour until it is in (or when the account has none).

use std::io::Cursor;

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, Context, Painter, Pos2, Stroke, TextureHandle, TextureOptions, pos2, vec2};

/// The picture's side in pixels (drawn at most 40 points wide on a double-density screen).
const SIDE: u32 = 96;
/// The most bytes read and the most pixels on a side: what the server sends is small, but it is
/// still bytes from the network.
const MAX_BYTES: usize = 512 * 1024;
const MAX_SIDE: u32 = 4096;
/// Corners of the circle's outline.
const SEGMENTS: usize = 48;

/// The picture in `bytes` (JPEG, PNG, WebP or GIF), cut to a square and made [`SIDE`] pixels wide;
/// `None` when it is not one or is too large.
pub fn decode(ctx: &Context, bytes: &[u8]) -> Option<TextureHandle> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(128 << 20);
    reader.limits(limits);
    let picture = reader.decode().ok()?.resize_to_fill(SIDE, SIDE, image::imageops::FilterType::Lanczos3).to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw());
    Some(ctx.load_texture("account-picture", image, TextureOptions::LINEAR))
}

/// `texture` as a circle of radius `r` around `center`, with a hairline `ring` around it.
pub fn paint(painter: &Painter, center: Pos2, r: f32, texture: &TextureHandle, ring: Color32) {
    let mut mesh = Mesh::with_texture(texture.id());
    mesh.vertices.push(Vertex { pos: center, uv: pos2(0.5, 0.5), color: Color32::WHITE });
    for i in 0..=SEGMENTS {
        let a = std::f32::consts::TAU * i as f32 / SEGMENTS as f32;
        let dir = vec2(a.cos(), a.sin());
        mesh.vertices.push(Vertex { pos: center + dir * r, uv: pos2(0.5, 0.5) + dir * 0.5, color: Color32::WHITE });
    }
    for i in 0..SEGMENTS as u32 {
        mesh.indices.extend([0, i + 1, i + 2]);
    }
    painter.add(mesh);
    painter.circle_stroke(center, r, Stroke::new(1.0, ring));
}

/// Remember `texture` (or forget it) as the picture of the person at this screen, for the
/// avatars drawn by painters that have no [`FigApp`](crate::FigApp) to ask.
pub fn remember(ctx: &Context, texture: Option<TextureHandle>) {
    let id = egui::Id::unique("own_account_picture");
    ctx.data_mut(|d| {
        if let Some(t) = texture {
            d.insert_temp(id, t);
        } else {
            d.remove::<TextureHandle>(id);
        }
    });
}

/// The picture of the person at this screen, once the page's server has handed it over.
pub fn own(ctx: &Context) -> Option<TextureHandle> {
    ctx.data(|d| d.get_temp::<TextureHandle>(egui::Id::unique("own_account_picture")))
}
