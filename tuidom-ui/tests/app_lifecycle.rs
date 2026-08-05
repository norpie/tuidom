//! `App` wiring and teardown — F7 Step 3, exercised headlessly through the mount/teardown seam.
//!
//! Like `off_thread_flush`, this registers the process-global flush waker, so it lives as its own
//! integration binary to stay isolated from the parallel in-crate unit suite. It drives
//! [`App::mount_on`] — the headless-testable half of [`App::run`] — with a [`HeadlessRuntime`],
//! pumping the deferred flushes itself in place of the terminal loop.

use std::thread;

use tuidom::Document;
use tuidom::headless::HeadlessRuntime;
use tuidom_ui::{App, dyn_text, state};

#[test]
fn app_wires_off_thread_writes_and_tears_the_root_scope_down() {
    let doc = Document::new().unwrap();
    let value = state(0);

    // The root reads `value`; App::mount_on registers the flush hook and waker and mounts it.
    let app = App::new(move || dyn_text(move || format!("{}", value.get())));
    let session = app.mount_on(&doc).unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 8, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");
    assert_eq!(
        doc.get_children(doc.root()).len(),
        1,
        "the root view mounted one node under the document root"
    );

    // A write from another thread reaches the DOM only once the queued flush is pumped — proof
    // App wired both halves of the off-thread path.
    let writer = value;
    thread::spawn(move || writer.set(5)).join().unwrap();
    rt.render().unwrap();
    assert_eq!(
        rt.get_cell(0, 0).unwrap().text,
        "0",
        "off-thread write is not yet visible before the pump"
    );
    rt.pump();
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "5");

    // Tearing the app down disposes the root scope — nodes freed back to the root only. Spawned
    // tasks are aborted on the same dispose path (proven at the scope layer in the runtime tests).
    session.close();
    assert_eq!(
        doc.get_children(doc.root()).len(),
        0,
        "teardown disposed the root scope and removed its nodes"
    );

    // With the waker unregistered, a late off-thread write falls back to the inline path and finds
    // no live subscriber — harmless, and the pump drains nothing.
    let late = value;
    thread::spawn(move || late.set(9)).join().unwrap();
    rt.pump();
    rt.render().unwrap();
    assert_eq!(
        doc.get_children(doc.root()).len(),
        0,
        "a post-teardown write resurrects nothing"
    );
}
