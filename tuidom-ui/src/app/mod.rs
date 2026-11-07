//! `App` — the entry point that owns the document, wires the reactive runtime to it, mounts the
//! root, runs the engine loop, and disposes everything on shutdown.
//!
//! `App` is the seam where the two flush plumbings meet. Each app registers a runtime session:
//! the session's waker calls the engine's `request_runtime_flush`, and the engine's flush hook
//! drains that session — so a reactive write from a spawned task or a foreign thread reaches
//! the DOM on the next event-task flush, while in-listener writes stay same-frame (see the
//! [off-thread write contract](App#off-thread-writes)). Sessions are independent, so several
//! apps on headless documents can share a process, as parallel tests do.
//!
//! The wiring and teardown live in [`App::mount_on`] / [`AppSession`], a headless-testable unit
//! independent of the terminal. [`App::run`] is thin glue: create a document, wire, run the
//! engine loop, tear down.

use std::io;
use std::sync::Arc;

use tuidom::{Document, Result};

use crate::runtime::{Scope, SessionId, flush_session, register_session, remove_session};
use crate::view::{Mounted, View, component, mount_owned};

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
/// that path up. State created outside any app has no event task to defer to, so an off-thread
/// write to it runs its effects on the writing thread.
pub struct App {
    root: Box<dyn FnOnce() -> View + Send + Sync>,
    on_quit: Option<Box<dyn FnOnce() + Send>>,
}

impl App {
    /// Build an app from a root closure producing the view mounted under the document root.
    pub fn new<F, V>(root: F) -> Self
    where
        F: FnOnce() -> V + Send + Sync + 'static,
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

    /// Wire this app onto `doc` without running the terminal loop: register a runtime session
    /// whose waker asks `doc` for a flush, point the engine's flush hook at that session's drain,
    /// and mount the root under `doc.root()`.
    ///
    /// The returned [`AppSession`] owns the mounted root; dropping it (or calling
    /// [`close`](AppSession::close)) disposes the root scope and removes the session. This is
    /// the headless-testable half of [`run`](App::run): a test drives it with a
    /// [`HeadlessRuntime`](tuidom::headless::HeadlessRuntime) and
    /// [`pump`](tuidom::headless::HeadlessRuntime::pump)s the deferred flushes itself.
    pub fn mount_on(self, doc: &Document) -> Result<AppSession> {
        // The waker holds a document clone, dropped when the session is removed. The hook
        // captures only the id, so after teardown it finds no session and drains nothing.
        let waker_doc = doc.clone();
        let session = register_session(Arc::new(move || waker_doc.request_runtime_flush()));
        doc.set_runtime_flush_hook(Arc::new(move || flush_session(session)));

        let root = Scope::for_session(session);
        // Mounted as a component so the root closure runs under the session's scope, inside the
        // mount's inline span, the same way any component body does.
        match mount_owned(doc, doc.root(), component(self.root), &root) {
            Ok(mounted) => Ok(AppSession {
                mounted: Some(mounted),
                root: Some(root),
                session,
            }),
            Err(err) => {
                root.dispose();
                remove_session(session);
                Err(err)
            }
        }
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

/// A wired, mounted app minus the terminal loop: the root mount plus its runtime session.
/// Disposes the root scope and removes the session when closed or dropped.
///
/// Obtained from [`App::mount_on`]. Disposing the root scope tears the whole reactive tree down —
/// child scopes first, cleanups, spawned tasks aborted, then signals, effects, and nodes freed —
/// after which a late write from a task that outran its abort lands on a freed slot and is the
/// existing silent no-op.
pub struct AppSession {
    mounted: Option<Mounted>,
    root: Option<Scope>,
    session: SessionId,
}

impl AppSession {
    /// Tear the app down: dispose the root scope and remove the runtime session.
    ///
    /// Equivalent to dropping the session; provided so teardown can read as a deliberate step.
    pub fn close(mut self) {
        self.teardown();
    }

    fn teardown(&mut self) {
        // Dispose before removing the session, so a cleanup's writes still route somewhere valid.
        // `remove_session` is idempotent, so a `close` followed by drop is safe.
        if let Some(mounted) = self.mounted.take() {
            mounted.unmount();
        }
        if let Some(root) = self.root.take() {
            root.dispose();
        }
        remove_session(self.session);
    }
}

impl Drop for AppSession {
    fn drop(&mut self) {
        self.teardown();
    }
}

#[cfg(test)]
mod tests;
