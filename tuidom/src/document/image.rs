use std::sync::Arc;

use crate::canvas::CanvasBuffer;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;
use crate::image::{
    ASSUMED_CELL_PIXELS, HalfBlockImage, ImageData, ImageDrawing, ImageFit, ImageFitting,
    ImageProtocol, fit_image,
};
use crate::lock;
use crate::node::{NodeData, NodeKind};

use super::Document;

impl Document {
    /// Create a new image node, detached from the tree.
    ///
    /// An axis its style leaves unset is sized by the image: its pixels divided by the
    /// terminal's cell size, or an assumed 8 by 16 pixel cell where the terminal has not
    /// reported one. When the style sets just one axis, the other keeps the image's aspect
    /// ratio. Like a canvas, it can shrink below that size, takes no children, and starts
    /// with an empty image.
    ///
    /// Returns the [`NodeId`] of the created node.
    pub fn create_image(&self) -> Result<NodeId> {
        let id = self.inner.alloc(NodeData::image());
        if let Err(err) = self.register_layout_node(id) {
            self.inner.nodes.remove(&id);
            return Err(err);
        }
        Ok(id)
    }

    /// Replace an image node's image.
    ///
    /// The same `Arc` as the node already has is a no-op, decided without comparing pixels,
    /// so keep the `Arc` to show one image again.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an image node.
    pub fn set_image(&self, node: NodeId, image: Arc<ImageData>) -> Result<()> {
        let resized = {
            let Some(mut data) = self.inner.nodes.get_mut(&node) else {
                return Err(TuidomError::NodeNotFound { id: node });
            };
            let NodeKind::Image { data: current, .. } = &mut data.kind else {
                return Err(TuidomError::NodeNotImage { id: node });
            };
            if Arc::ptr_eq(current, &image) {
                return Ok(());
            }
            let resized = (current.width(), current.height()) != (image.width(), image.height());
            *current = image;
            resized
        };

        if resized {
            self.register_layout_node(node)?;
        }
        self.inner.notify.notify_one();
        Ok(())
    }

    /// Set how an image node's image fills its content box. [`ImageFit::Contain`] by default.
    ///
    /// # Errors
    ///
    /// Returns an error if `node` does not exist or is not an image node.
    pub fn set_image_fit(&self, node: NodeId, fit: ImageFit) -> Result<()> {
        {
            let Some(mut data) = self.inner.nodes.get_mut(&node) else {
                return Err(TuidomError::NodeNotFound { id: node });
            };
            let NodeKind::Image { fit: current, .. } = &mut data.kind else {
                return Err(TuidomError::NodeNotImage { id: node });
            };
            if *current == fit {
                return Ok(());
            }
            *current = fit;
        }
        self.inner.notify.notify_one();
        Ok(())
    }

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

    /// The half-block cells an image node shows in a content box of `content` cells, made
    /// fresh only when its image, fitted size or crop changed since the last frame.
    pub(crate) fn half_block_image(
        &self,
        node: NodeId,
        image: &Arc<ImageData>,
        fit: ImageFit,
        content: (u16, u16),
        cell: (u16, u16),
    ) -> Option<(ImageFitting, Arc<CanvasBuffer>)> {
        let mut cache = lock::mutex(&self.inner.half_block_images);
        let Some(fitting) = fit_image((image.width(), image.height()), cell, content, fit) else {
            cache.remove(&node);
            return None;
        };
        let entry = HalfBlockImage::for_fitting(cache.get(&node), image, fitting);
        let cells = entry.cells().clone();
        cache.insert(node, entry);
        Some((fitting, cells))
    }

    /// Drop every cached half-block image, for frames that draw images another way.
    pub(crate) fn clear_half_block_images(&self) {
        lock::mutex(&self.inner.half_block_images).clear();
    }

    /// One cell's size in pixels: the terminal's where it reported one, assumed otherwise.
    pub(crate) fn cell_pixels(&self) -> (u16, u16) {
        lock::mutex(&self.inner.terminal_info)
            .as_ref()
            .and_then(|info| info.pixel_size)
            .map(|size| (size.cell_width, size.cell_height))
            .filter(|&(width, height)| width > 0 && height > 0)
            .unwrap_or(ASSUMED_CELL_PIXELS)
    }
}
