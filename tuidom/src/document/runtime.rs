//! Framework-facing runtime plumbing.
//!
//! These methods are the seam a reactive framework wires itself into: they let it run its own
//! flush on the event task without the engine knowing anything about reactivity. The engine
//! holds an opaque callback and a way to ask for it to run — nothing more.

use std::sync::Arc;

use crate::document::Document;
use crate::lock;
use crate::runtime_event::RuntimeEvent;

impl Document {
    /// Register the flush hook run on the event task for each [`request_runtime_flush`].
    ///
    /// The engine treats the hook as opaque: it stores it and calls it, never inspecting what
    /// it does. A reactive framework registers one once at startup to drain its own pending
    /// work — running effects that mutate the DOM — serialized with input dispatch. Registering
    /// again replaces the previous hook.
    ///
    /// This is framework plumbing, not general application API. Building a UI on tuidom does not
    /// require touching it.
    ///
    /// [`request_runtime_flush`]: Self::request_runtime_flush
    pub fn set_runtime_flush_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) {
        *lock::rw_write(&self.inner.flush_hook) = Some(hook);
    }

    /// Enqueue a flush request, running the registered hook on the event task.
    ///
    /// Callable from any thread — a spawned task or a foreign thread that made a reactive write
    /// off the event task calls this to have that write reach the DOM on the next drain. The
    /// hook runs on the event task, serialized with input dispatch, never on the calling thread.
    ///
    /// A no-op when no hook is registered or the runtime has shut down. This is framework
    /// plumbing, not general application API.
    pub fn request_runtime_flush(&self) {
        // Mirrors the runtime's other cross-thread enqueues: a closed queue means the loop is
        // gone, which is not an error worth surfacing to the caller.
        let _ = self.inner.event_tx.send(RuntimeEvent::Flush);
    }
}
