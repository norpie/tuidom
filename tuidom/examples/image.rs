//! An image in a bordered box, for checking how each terminal draws it. With a path argument it
//! shows that file; without one, a generated picture: a hue gradient with a checkerboard corner
//! and a soft transparent hole, so color, fitting and alpha all show.
//!
//! f — cycle the fit (contain, cover, fill)
//! p — cycle the protocol (auto, kitty, half-block)
//! `+` and `-` — widen or narrow the box
//! q — quit

use std::sync::{Arc, Mutex};

use tuidom::event::KeyCode;
use tuidom::image::{ImageData, ImageFit, ImageProtocol};
use tuidom::style::{Border, BorderCharset, Length, Style};
use tuidom::{Document, NodeId, Result, style};

const FITS: [ImageFit; 3] = [ImageFit::Contain, ImageFit::Cover, ImageFit::Fill];
const PROTOCOLS: [ImageProtocol; 3] = [
    ImageProtocol::Auto,
    ImageProtocol::Kitty,
    ImageProtocol::HalfBlock,
];

#[derive(Clone, Copy)]
struct Settings {
    fit: usize,
    protocol: usize,
    width: u16,
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let data = match std::env::args().nth(1) {
        Some(path) => ImageData::from_bytes(&std::fs::read(path)?)?,
        None => generated(320, 240)?,
    };

    let doc = Document::new()?;
    doc.set_style(doc.root(), &style! { flex_direction: column })?;

    let status = doc.create_text("")?;
    doc.append_child(doc.root(), status)?;

    let frame = doc.create_box()?;
    let mut frame_style = Style::new();
    frame_style.flex_grow(1.0);
    frame_style.border(Border::new(BorderCharset::rounded()));
    doc.set_style(frame, &frame_style)?;
    doc.append_child(doc.root(), frame)?;

    let image = doc.create_image()?;
    doc.set_style(image, &style! { flex_grow: 1.0 })?;
    doc.set_image(image, Arc::new(data))?;
    doc.append_child(frame, image)?;

    let settings = Arc::new(Mutex::new(Settings {
        fit: 0,
        protocol: 0,
        width: 100,
    }));
    apply(&doc, frame, image, status, *settings.lock().unwrap())?;

    let d = doc.clone();
    let current = settings.clone();
    doc.on_terminal_info(move |_| {
        let _ = apply(&d, frame, image, status, *current.lock().unwrap());
    });

    let d = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        let mut settings = settings.lock().unwrap();
        match key.code {
            KeyCode::Char('q') => return d.quit(),
            KeyCode::Char('f') => settings.fit = (settings.fit + 1) % FITS.len(),
            KeyCode::Char('p') => settings.protocol = (settings.protocol + 1) % PROTOCOLS.len(),
            KeyCode::Char('+') => settings.width = (settings.width + 10).min(100),
            KeyCode::Char('-') => settings.width = settings.width.saturating_sub(10).max(20),
            _ => return,
        }
        let _ = apply(&d, frame, image, status, *settings);
    })?;

    doc.run().await?;
    Ok(())
}

fn apply(
    doc: &Document,
    frame: NodeId,
    image: NodeId,
    status: NodeId,
    settings: Settings,
) -> Result<()> {
    let fit = FITS[settings.fit];
    let protocol = PROTOCOLS[settings.protocol];
    doc.set_image_fit(image, fit)?;
    doc.set_image_protocol(protocol);
    doc.update_style(frame, |style| {
        style.width(Length::Percent(f64::from(settings.width)));
    })?;

    let info = doc.terminal();
    let drawn = match protocol {
        ImageProtocol::Auto => {
            if info.as_ref().is_some_and(|info| info.graphics.placeholders) {
                "auto (placeholders)"
            } else {
                "auto (half-blocks)"
            }
        }
        ImageProtocol::Kitty => "kitty",
        ImageProtocol::Sixel => "sixel",
        ImageProtocol::ITerm2 => "iterm2",
        ImageProtocol::HalfBlock => "half-block",
    };
    let cell = info.and_then(|info| info.pixel_size).map_or_else(
        || "8×16 px (assumed)".to_string(),
        |size| format!("{}×{} px", size.cell_width, size.cell_height),
    );
    doc.set_text_content(
        status,
        format!(
            "fit: {fit:?}  protocol: {drawn}  cell: {cell}  width: {}%  [f p + - q]",
            settings.width
        ),
    )
}

/// A hue gradient darkening downward, a checkerboard in the top-left corner, and a hole in the
/// middle that fades to transparent.
fn generated(width: u32, height: u32) -> Result<ImageData> {
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    let (center_x, center_y) = (width as f32 / 2.0, height as f32 / 2.0);
    let radius = height as f32 / 4.0;
    for y in 0..height {
        for x in 0..width {
            let [r, g, b] = if x < 96 && y < 96 {
                let shade = if (x / 16 + y / 16) % 2 == 0 { 240 } else { 30 };
                [shade; 3]
            } else {
                let hue = x as f32 / width as f32 * 360.0;
                hsv(hue, 0.75, 1.0 - y as f32 / height as f32 * 0.5)
            };
            let distance = ((x as f32 - center_x).powi(2) + (y as f32 - center_y).powi(2)).sqrt();
            let alpha = ((distance - radius) / 16.0).clamp(0.0, 1.0);
            rgba.extend([r, g, b, (alpha * 255.0).round() as u8]);
        }
    }
    ImageData::from_rgba(width, height, rgba)
}

fn hsv(hue: f32, saturation: f32, value: f32) -> [u8; 3] {
    let chroma = value * saturation;
    let sector = hue / 60.0;
    let second = chroma * (1.0 - (sector % 2.0 - 1.0).abs());
    let (r, g, b) = match sector as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = value - chroma;
    [r, g, b].map(|channel| ((channel + base) * 255.0).round() as u8)
}
