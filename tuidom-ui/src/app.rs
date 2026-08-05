//! `App` — the entry point that owns the document, wires the reactive runtime to it, mounts the
//! root, runs the engine loop, and disposes everything on shutdown.
//!
//! `App` is the seam where the two flush plumbings meet. The engine's flush hook is pointed at
//! the runtime's [`flush_deferred`], and the runtime's waker at the engine's
//! `request_runtime_flush` — so a reactive write from a spawned task or a foreign thread reaches
//! the DOM on the next event-task flush, while in-listener writes stay same-frame (see the
//! [off-thread write contract](App#off-thread-writes)).
//!
//! The wiring and teardown live in [`App::mount_on`] / [`AppSession`], a headless-testable unit
//! independent of the terminal. [`App::run`] is thin glue: create a document, wire, run the
//! engine loop, tear down.

use std::io;
use std::sync::Arc;

use tuidom::{Document, Result};

use crate::runtime::{clear_flush_waker, flush_deferred, set_flush_waker};
use crate::view::{Mounted, View, mount};

/// The application entry point.
///
/// Build one from a root closure, then [`run`](App::run) it:
///
/// ```ignore
/// App::new(|| my_root_view())
///     .on_quit(|| save_state())
///     .run()
///     .await?;
/// ```
///
/// The root closure runs once, building the view mounted under the document root — exactly like
/// any component body. Everything it creates is owned by the root scope and torn down together on
/// shutdown.
///
/// # Off-thread writes
///
/// A reactive write made on the event task — inside a listener, during mount, from an `on_mount`
/// callback — flushes in the same frame. A write made off the event task — from a task spawned
/// with [`Scope::spawn`](crate::Scope::spawn), or a foreign thread — cannot run effects there, so
/// it is deposited for the event task and reaches the DOM on the next flush. `App` is what wires
/// that path up; without it, off-thread writes mark state dirty but never flush.
pub struct App {
    root: Box<dyn FnOnce() -> View + Send>,
    on_quit: Option<Box<dyn FnOnce() + Send>>,
}

impl App {
    /// Build an app from a root closure producing the view mounted under the document root.
    pub fn new<F, V>(root: F) -> Self
    where
        F: FnOnce() -> V + Send + 'static,
        V: Into<View>,
    {
        App {
            root: Box::new(move || root().into()),
            on_quit: None,
        }
    }

    /// Register a callback to run once the engine loop returns — after [`quit`](Document::quit),
    /// before the root scope is disposed, so it can still read the app's signals.
    pub fn on_quit<F>(mut self, f: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        self.on_quit = Some(Box::new(f));
        self
    }

    /// Wire this app onto `doc` without running the terminal loop: point the engine's flush hook
    /// at the runtime drain, register the waker, and mount the root under `doc.root()`.
    ///
    /// The returned [`AppSession`] owns the mounted root; dropping it (or calling
    /// [`close`](AppSession::close)) disposes the root scope and unregisters the waker. This is
    /// the headless-testable half of [`run`](App::run): a test drives it with a
    /// [`HeadlessRuntime`](tuidom::headless::HeadlessRuntime) and
    /// [`pump`](tuidom::headless::HeadlessRuntime::pump)s the deferred flushes itself.
    pub fn mount_on(self, doc: &Document) -> Result<AppSession> {
        // Point the engine's flush hook at the runtime drain, and the runtime waker at the
        // engine's cross-thread enqueue. `flush_deferred` captures nothing, so the engine holds
        // no framework state through it; the waker holds a document clone, dropped on teardown.
        doc.set_runtime_flush_hook(Arc::new(flush_deferred));
        let waker_doc = doc.clone();
        set_flush_waker(Arc::new(move || waker_doc.request_runtime_flush()));

        let mounted = mount(doc, doc.root(), (self.root)())?;
        Ok(AppSession {
            mounted: Some(mounted),
        })
    }

    /// Create a document, wire the app onto it, run the engine loop until
    /// [`quit`](Document::quit), then fire `on_quit` and dispose the root scope.
    pub async fn run(mut self) -> io::Result<()> {
        let on_quit = self.on_quit.take();
        let doc = Document::new().map_err(io::Error::other)?;
        let session = self.mount_on(&doc).map_err(io::Error::other)?;

        let result = doc.clone().run().await;

        if let Some(on_quit) = on_quit {
            on_quit();
        }
        session.close();
        result
    }
}

/// A wired, mounted app minus the terminal loop: the root mount plus the runtime waker
/// registration. Disposes the root scope and unregisters the waker when closed or dropped.
///
/// Obtained from [`App::mount_on`]. Disposing the root scope tears the whole reactive tree down —
/// child scopes first, cleanups, spawned tasks aborted, then signals, effects, and nodes freed —
/// after which a late write from a task that outran its abort lands on a freed slot and is the
/// existing silent no-op.
pub struct AppSession {
    mounted: Option<Mounted>,
}

impl AppSession {
    /// Tear the app down: dispose the root scope and unregister the runtime waker.
    ///
    /// Equivalent to dropping the session; provided so teardown can read as a deliberate step.
    pub fn close(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        // Dispose first so any cleanup can still touch the runtime, then unregister the waker so a
        // write racing the teardown falls back to the inline path rather than a mailbox nothing
        // drains. `clear_flush_waker` is idempotent, so a `close` followed by drop is safe.
        if let Some(mounted) = self.mounted.take() {
            mounted.unmount();
        }
        clear_flush_waker();
    }
}

impl Drop for AppSession {
    fn drop(&mut self) {
        self.teardown();
    }
}
