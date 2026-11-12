//! A canvas that fills the screen and redraws whenever its size changes: a dotted frame, a sine
//! wave across the width, and its size in the middle. Resize the terminal to watch it follow.
//!
//! q — quit

use std::f32::consts::TAU;

use tuidom::canvas::{CanvasBuffer, CanvasStyle};
use tuidom::event::KeyCode;
use tuidom::style::ResolvedColor;
use tuidom::{Document, style};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = Document::new()?;

    let canvas = doc.create_canvas()?;
    doc.set_style(canvas, &style! { flex_grow: 1.0 })?;
    doc.append_child(doc.root(), canvas)?;

    let d = doc.clone();
    doc.on_size_change(canvas, move |event| {
        let _ = d.set_canvas(canvas, draw(event.width, event.height));
    })?;

    let d = doc.clone();
    doc.on_key_press(doc.root(), move |key| {
        if key.code == KeyCode::Char('q') {
            d.quit();
        }
    })?;

    doc.run().await?;
    Ok(())
}

fn draw(width: u16, height: u16) -> CanvasBuffer {
    let mut buffer = CanvasBuffer::new(width, height);
    if width < 3 || height < 3 {
        return buffer;
    }

    let frame = CanvasStyle {
        fg: Some(ResolvedColor::oklch(0.5, 0.02, 270.0)),
        ..CanvasStyle::default()
    };
    for x in 0..width {
        buffer.set(x, 0, "·", frame);
        buffer.set(x, height - 1, "·", frame);
    }
    for y in 0..height {
        buffer.set(0, y, "·", frame);
        buffer.set(width - 1, y, "·", frame);
    }

    let inner_width = f32::from(width - 2);
    let middle = f32::from(height - 1) / 2.0;
    let amplitude = (f32::from(height) - 3.0) / 2.0;
    for x in 1..width - 1 {
        let t = f32::from(x - 1) / inner_width;
        let y = (middle - amplitude * (t * TAU).sin()).round() as u16;
        let wave = CanvasStyle {
            fg: Some(ResolvedColor::oklch(
                0.75,
                0.15,
                260.0 - 230.0 * f64::from(t),
            )),
            bold: true,
            ..CanvasStyle::default()
        };
        buffer.set(x, y, "•", wave);
    }

    let label = format!(" {width}×{height} ");
    let label_width = label.chars().count() as u16;
    if label_width < width {
        let label_style = CanvasStyle {
            fg: Some(ResolvedColor::oklch(0.2, 0.02, 270.0)),
            bg: Some(ResolvedColor::oklch(0.85, 0.13, 90.0)),
            ..CanvasStyle::default()
        };
        buffer.text((width - label_width) / 2, height / 2, &label, label_style);
    }
    buffer
}
