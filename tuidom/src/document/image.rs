use crate::image::{ImageDrawing, ImageProtocol};
use crate::lock;

use super::Document;

impl Document {
    /// Choose how images are drawn. [`ImageProtocol::Auto`] by default.
    pub fn set_image_protocol(&self, protocol: ImageProtocol) {
        let mut current = lock::mutex(&self.inner.image_protocol);
        if *current == protocol {
            return;
        }
        *current = protocol;
        drop(current);
        self.inner.notify.notify_one();
    }

    /// How images are drawn.
    pub fn image_protocol(&self) -> ImageProtocol {
        *lock::mutex(&self.inner.image_protocol)
    }

    pub(crate) fn image_drawing(&self) -> ImageDrawing {
        match self.image_protocol() {
            ImageProtocol::Kitty => ImageDrawing::Placeholders,
            ImageProtocol::HalfBlock => ImageDrawing::HalfBlock,
            ImageProtocol::Auto => {
                let placeholders = lock::mutex(&self.inner.terminal_info)
                    .as_ref()
                    .is_some_and(|info| info.graphics.placeholders);
                if placeholders {
                    ImageDrawing::Placeholders
                } else {
                    ImageDrawing::HalfBlock
                }
            }
        }
    }
}
