//! `App` wiring, driven headlessly through [`App::mount_on`]. A [`HeadlessRuntime`] stands in for
//! the terminal loop, and [`pump`](HeadlessRuntime::pump) services the flushes off-task writes
//! request.
//!
//! Each app registers its own runtime session, so these run alongside the rest of the suite.

use std::sync::{Arc, Mutex};
use std::thread;

use tokio::sync::mpsc;
use tuidom::Document;
use tuidom::headless::HeadlessRuntime;

use super::App;
use crate::{dyn_text, effect, spawn, state};

/// The rendered first row, trailing blanks trimmed.
fn row0(rt: &HeadlessRuntime) -> String {
    (0..rt.width() as i32)
        .filter_map(|x| rt.get_cell(x, 0).map(|c| c.text))
        .collect::<String>()
        .trim_end()
        .to_string()
}

#[test]
fn an_off_thread_write_reaches_the_dom_on_the_next_pump() {
    let doc = Document::new().unwrap();
    let value = state(0);

    // Records the thread each effect run lands on, to prove the rerun happens on the pump side
    // and never on the writer.
    let run_threads: Arc<Mutex<Vec<thread::ThreadId>>> = Arc::new(Mutex::new(Vec::new()));
    let probe = run_threads.clone();
    let _session = App::new(move || {
        effect(move || {
            value.get();
            probe.lock().unwrap().push(thread::current().id());
        });
        dyn_text(move || format!("{}", value.get()))
    })
    .mount_on(&doc)
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 8, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "0");

    let runs_before = run_threads.lock().unwrap().len();
    let main_id = thread::current().id();
    let writer_id = thread::spawn(move || {
        value.set(7);
        thread::current().id()
    })
    .join()
    .unwrap();

    assert_eq!(
        run_threads.lock().unwrap().len(),
        runs_before,
        "the effect did not rerun on the writing thread"
    );
    rt.render().unwrap();
    assert_eq!(row0(&rt), "0", "not visible until the flush is pumped");

    rt.pump();
    rt.render().unwrap();
    assert_eq!(row0(&rt), "7");

    let threads = run_threads.lock().unwrap();
    assert!(threads.len() > runs_before, "the effect reran on pump");
    assert!(!threads.contains(&writer_id));
    assert!(threads.iter().all(|id| *id == main_id));
}

#[test]
fn closing_the_session_disposes_the_root_and_stops_routing() {
    let doc = Document::new().unwrap();
    let value = state(0);
    let session = App::new(move || dyn_text(move || format!("{}", value.get())))
        .mount_on(&doc)
        .unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 8, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "0");
    assert_eq!(doc.get_children(doc.root()).len(), 1);

    thread::spawn(move || value.set(5)).join().unwrap();
    rt.pump();
    rt.render().unwrap();
    assert_eq!(row0(&rt), "5");

    session.close();
    assert_eq!(doc.get_children(doc.root()).len(), 0);

    // The binding effect is freed with the root, so a late write reaches nothing, and the
    // engine hook finds no session to drain.
    thread::spawn(move || value.set(9)).join().unwrap();
    rt.pump();
    rt.render().unwrap();
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}

#[test]
fn two_apps_in_one_process_flush_independently() {
    let (doc_a, doc_b) = (Document::new().unwrap(), Document::new().unwrap());
    let (a, b) = (state(0), state(0));
    let session_a = App::new(move || dyn_text(move || format!("{}", a.get())))
        .mount_on(&doc_a)
        .unwrap();
    let session_b = App::new(move || dyn_text(move || format!("{}", b.get())))
        .mount_on(&doc_b)
        .unwrap();

    let mut rt_a = HeadlessRuntime::new(doc_a, 4, 1);
    let mut rt_b = HeadlessRuntime::new(doc_b, 4, 1);

    thread::spawn(move || {
        a.set(1);
        b.set(2);
    })
    .join()
    .unwrap();

    // Each document's queue holds its own flush, and pumping one leaves the other pending.
    rt_a.pump();
    rt_a.render().unwrap();
    rt_b.render().unwrap();
    assert_eq!(row0(&rt_a), "1");
    assert_eq!(row0(&rt_b), "0");

    // Closing A leaves B wired.
    session_a.close();
    rt_b.pump();
    rt_b.render().unwrap();
    assert_eq!(row0(&rt_b), "2");

    thread::spawn(move || b.set(3)).join().unwrap();
    rt_b.pump();
    rt_b.render().unwrap();
    assert_eq!(row0(&rt_b), "3");
    session_b.close();
}

/// The tri-state a streamed resource moves through: the shape the resource pattern documents
/// for user code over `spawn` and a signal.
#[derive(Clone, Copy, Default, PartialEq)]
enum Resource {
    #[default]
    Loading,
    Ready(i32),
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streamed_resource_repaints_as_values_arrive_then_disposes_cleanly() {
    let doc = Document::new().unwrap();

    // Values the test feeds, and an ack per value, so each off-task write has landed (and queued
    // its flush) before the test pumps — no reliance on wall-clock timing.
    let (value_tx, mut value_rx) = mpsc::unbounded_channel::<i32>();
    let (ack_tx, mut ack_rx) = mpsc::unbounded_channel::<()>();

    let session = App::new(move || {
        let resource = state(Resource::Loading);
        // Owned by the root's scope, so teardown aborts it.
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
    .mount_on(&doc)
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc.clone(), 12, 1);
    rt.render().unwrap();
    assert_eq!(row0(&rt), "loading");

    for v in [42, 7] {
        value_tx.send(v).unwrap();
        ack_rx.recv().await.unwrap();
        rt.pump();
        rt.render().unwrap();
        assert_eq!(row0(&rt), format!("value {v}"));
    }

    session.close();
    assert_eq!(doc.get_children(doc.root()).len(), 0);

    // The task is aborted, so a further value reaches nothing. The send may fail once the
    // aborted task drops its receiver; either way nothing repaints.
    let _ = value_tx.send(99);
    rt.pump();
    rt.render().unwrap();
    assert_eq!(doc.get_children(doc.root()).len(), 0);
}
