use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::id::NodeId;

impl Document {
    /// Set a string attribute on a node.
    ///
    /// Attribute keys must be non-empty. Attribute mutations notify the render
    /// task so downstream code that observes rendered frames can react to the
    /// same mutation signal as other document changes.
    ///
    /// # Errors
    ///
    /// Returns [`TuidomError::InvalidAttributeKey`] if `key` is empty.
    /// Returns [`TuidomError::NodeNotFound`] if `node` does not exist.
    pub fn set_attr(
        &self,
        node: NodeId,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<()> {
        let key = key.into();
        if key.is_empty() {
            return Err(TuidomError::InvalidAttributeKey);
        }

        let Some(mut data) = self.inner.nodes.get_mut(&node) else {
            return Err(TuidomError::NodeNotFound { id: node });
        };
        data.attrs.insert(key, value.into());
        drop(data);

        self.inner.notify.notify_one();
        Ok(())
    }

    /// Get a string attribute from a node.
    ///
    /// Attribute keys must be non-empty.
    ///
    /// # Errors
    ///
    /// Returns [`TuidomError::InvalidAttributeKey`] if `key` is empty.
    /// Returns [`TuidomError::NodeNotFound`] if `node` does not exist.
    pub fn get_attr(&self, node: NodeId, key: &str) -> Result<Option<String>> {
        if key.is_empty() {
            return Err(TuidomError::InvalidAttributeKey);
        }

        let Some(data) = self.inner.nodes.get(&node) else {
            return Err(TuidomError::NodeNotFound { id: node });
        };
        Ok(data.attrs.get(key).cloned())
    }

    /// Remove a string attribute from a node.
    ///
    /// Attribute keys must be non-empty. Removing a missing attribute is a
    /// no-op. Attribute mutations notify the render task so downstream code
    /// that observes rendered frames can react to the same mutation signal as
    /// other document changes.
    ///
    /// # Errors
    ///
    /// Returns [`TuidomError::InvalidAttributeKey`] if `key` is empty.
    /// Returns [`TuidomError::NodeNotFound`] if `node` does not exist.
    pub fn remove_attr(&self, node: NodeId, key: &str) -> Result<()> {
        if key.is_empty() {
            return Err(TuidomError::InvalidAttributeKey);
        }

        let Some(mut data) = self.inner.nodes.get_mut(&node) else {
            return Err(TuidomError::NodeNotFound { id: node });
        };
        data.attrs.remove(key);
        drop(data);

        self.inner.notify.notify_one();
        Ok(())
    }
}
