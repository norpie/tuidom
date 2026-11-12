//! The kitty graphics commands that put images behind placeholder cells.
//!
//! An id names one image at one fitted size and crop, uploaded already resampled to the pixels
//! its cells cover. Kitty fits an image into a placeholder placement keeping its aspect ratio,
//! so sending it at the placement's exact shape is what lets `Fill` stretch and `Cover` crop.

use std::collections::HashSet;
use std::sync::Arc;

use super::{ImageData, ImageFitting, base64, compress};

/// Base64 bytes per command; kitty takes at most 4096, and every chunk but the last must be a
/// multiple of four.
const CHUNK: usize = 4096;

/// An image a frame drew as placeholder cells.
#[derive(Debug, Clone)]
pub(crate) struct KittyShown {
    pub id: u32,
    pub image: Arc<ImageData>,
    pub fitting: ImageFitting,
    /// One cell's size in pixels when it was drawn.
    pub cell: (u16, u16),
}

/// The image ids the terminal holds, kept by whatever flushes frames to it.
#[derive(Debug, Default)]
pub(crate) struct KittyUploads {
    held: HashSet<u32>,
}

impl KittyUploads {
    /// The commands that bring the terminal in line with a frame: upload every id it shows
    /// that the terminal lacks, and delete every held id it does not show. Inside tmux each
    /// command is wrapped for passthrough.
    pub fn commands(&mut self, shown: &[KittyShown], tmux: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let showing: HashSet<u32> = shown.iter().map(|image| image.id).collect();
        let mut gone: Vec<u32> = self.held.difference(&showing).copied().collect();
        gone.sort_unstable();
        for id in gone {
            self.held.remove(&id);
            delete(&mut out, id, tmux);
        }
        for image in shown {
            if !self.held.contains(&image.id) && upload(&mut out, image, tmux) {
                self.held.insert(image.id);
            }
        }
        out
    }

    /// Delete every held id, for when the app stops drawing to the terminal.
    pub fn delete_all(&mut self, tmux: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let mut held: Vec<u32> = self.held.drain().collect();
        held.sort_unstable();
        for id in held {
            delete(&mut out, id, tmux);
        }
        out
    }
}

/// Queue the transmission of `image` with a virtual placement of its cell size. Returns whether
/// anything was queued.
fn upload(out: &mut Vec<u8>, shown: &KittyShown, tmux: bool) -> bool {
    let fitting = &shown.fitting;
    let width = u32::from(fitting.width) * u32::from(shown.cell.0.max(1));
    let height = u32::from(fitting.height) * u32::from(shown.cell.1.max(1));
    let pixels = shown.image.resample(fitting.source, width, height);
    let Some(compressed) = compress(pixels.as_flattened()) else {
        tracing::warn!("could not compress image {} for upload", shown.id);
        return false;
    };
    let payload = base64(&compressed);
    // Base64 is ASCII, so any byte offset is a character boundary.
    let chunks: Vec<&str> = (0..payload.len())
        .step_by(CHUNK)
        .map(|start| &payload[start..(start + CHUNK).min(payload.len())])
        .collect();
    let last = chunks.len().saturating_sub(1);
    for (index, data) in chunks.iter().enumerate() {
        let more = u8::from(index < last);
        let keys = if index == 0 {
            format!(
                "a=T,U=1,f=32,o=z,s={width},v={height},i={},c={},r={},q=2,m={more}",
                shown.id, fitting.width, fitting.height
            )
        } else {
            format!("m={more},q=2")
        };
        emit(out, &format!("\x1b_G{keys};{data}\x1b\\"), tmux);
    }
    true
}

/// Queue the deletion of image `id` and the data behind it.
fn delete(out: &mut Vec<u8>, id: u32, tmux: bool) {
    emit(out, &format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\"), tmux);
}

/// Append one escape sequence, wrapped for tmux passthrough when asked: tmux forwards the
/// body of `ESC P tmux; … ESC \` once every escape inside it is doubled.
fn emit(out: &mut Vec<u8>, sequence: &str, tmux: bool) {
    if tmux {
        out.extend_from_slice(b"\x1bPtmux;");
        out.extend_from_slice(sequence.replace('\x1b', "\x1b\x1b").as_bytes());
        out.extend_from_slice(b"\x1b\\");
    } else {
        out.extend_from_slice(sequence.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(id: u32) -> KittyShown {
        KittyShown {
            id,
            image: Arc::new(ImageData::from_rgba(1, 1, vec![255; 4]).unwrap()),
            fitting: ImageFitting {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
                source: (0, 0, 1, 1),
            },
            cell: (2, 4),
        }
    }

    #[test]
    fn deleting_everything_empties_what_is_held() {
        let mut uploads = KittyUploads::default();
        uploads.commands(&[shown(3), shown(1)], false);
        assert_eq!(
            uploads.delete_all(false),
            b"\x1b_Ga=d,d=I,i=1,q=2\x1b\\\x1b_Ga=d,d=I,i=3,q=2\x1b\\"
        );
        assert!(uploads.delete_all(false).is_empty());
        assert!(!uploads.commands(&[shown(1)], false).is_empty());
    }

    #[test]
    fn tmux_passthrough_doubles_every_escape() {
        let mut out = Vec::new();
        emit(&mut out, "\x1b_Ga=d\x1b\\", true);
        assert_eq!(out, b"\x1bPtmux;\x1b\x1b_Ga=d\x1b\x1b\\\x1b\\");
    }
}
