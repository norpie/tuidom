//! Worked example: a background task streaming a resource into the view — F7 Step 4, D7.
//!
//! This is the resource pattern the framework documents rather than ships: a tri-state signal
//! (`Loading` → `Ready`) that a task owned by [`spawn`](tuidom_ui::spawn) fills as data arrives,
//! repainting on each update, with the task aborted when the app is torn down. Nothing here is a
//! framework type — it is all user code over `spawn` + a signal.
//!
//! It runs headlessly: [`App::mount_on`] wires the app onto a document a [`HeadlessRuntime`] drives,
//! and `AppSession::close` stands in for "quit + dispose cleanly". Like the other off-thread tests
//! it registers the process-global waker, so it lives in its own integration binary.
//!
//! The stream is fed from the test through a channel, with an ack per item, so each off-event-task
//! write is observed deterministically: the ack means the write (and its queued flush) has landed,
//! and the pump then applies it — no reliance on wall-clock timing.

use tokio::sync::mpsc;

use tuidom::Document;
use tuidom::headless::HeadlessRuntime;
use tuidom_ui::{App, component, dyn_text, spawn, state};

/// The tri-state a streamed resource moves through — the shape D7 points users at.
///
/// `Default` (used by [`Signal::get`](tuidom_ui::Signal::get)'s freed-slot fallback) and
/// `PartialEq` (its equality-skip on write) are what a signal asks of any value it stores.
#[derive(Clone, Copy, Default, PartialEq)]
enum Resource {
    #[default]
    Loading,
    Ready(i32),
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streamed_resource_repaints_as_values_arrive_then_disposes_cleanly() {
    let doc = Document::new().unwrap();

    // The "stream": values the test feeds, and an ack per value so its arrival is deterministic.
    let (value_tx, mut value_rx) = mpsc::unbounded_channel::<i32>();
    let (ack_tx, mut ack_rx) = mpsc::unbounded_channel::<()>();

    let session = App::new(move || {
        component(move || {
            // The resource, owned by this component's scope.
            let resource = state(Resource::Loading);

            // The background task, owned by the same scope: unmount aborts it, so it can never
            // write into a freed signal. It writes off the event task, so each `set` defers and
            // reaches the DOM on the next pump.
            spawn(async move {
                while let Some(v) = value_rx.recv().await {
                    resource.set(Resource::Ready(v));
                    let _ = ack_tx.send(());
                }
            });

            dyn_text(move || match resource.get() {
                Resource::Loading => "loading".to_string(),
                Resource::Ready(v) => format!("value {v}"),
            })
        })
    })
    .mount_on(&doc)
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 12, 1);
    rt.render().unwrap();
    assert_eq!(read(&rt), "loading");

    // Each value: feed it, wait for the task's ack (the write has happened), pump, repaint.
    value_tx.send(42).unwrap();
    ack_rx.recv().await.unwrap();
    rt.pump();
    rt.render().unwrap();
    assert_eq!(read(&rt), "value 42");

    value_tx.send(7).unwrap();
    ack_rx.recv().await.unwrap();
    rt.pump();
    rt.render().unwrap();
    assert_eq!(read(&rt), "value 7");

    // "Quit + dispose cleanly": teardown aborts the task and frees the whole tree.
    session.close();
    assert_eq!(
        doc.get_children(doc.root()).len(),
        0,
        "teardown disposed the root scope"
    );

    // The task is aborted, so a further value reaches nothing — no repaint, no panic. (The send
    // itself may fail once the aborted task drops the receiver; either way nothing repaints.)
    let _ = value_tx.send(99);
    rt.pump();
    rt.render().unwrap();
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}

/// The rendered first row, trailing blanks trimmed.
fn read(rt: &HeadlessRuntime) -> String {
    (0..rt.width() as i32)
        .filter_map(|x| rt.get_cell(x, 0).map(|c| c.text))
        .collect::<String>()
        .trim_end()
        .to_string()
}
