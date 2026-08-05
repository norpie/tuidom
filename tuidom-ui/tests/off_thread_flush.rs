//! Off-thread reactive writes reaching the DOM — F7 Step 2, verified through the headless pump.
//!
//! This is the one tuidom-ui test that lives outside the crate. It registers the *process-global*
//! flush waker, which flips every write on this thread's runtime onto the deferred path. In-crate
//! it would share one runtime with the unit tests running in parallel and knock their inline
//! writes onto the mailbox mid-test. As its own integration binary it runs in its own process,
//! with a runtime of its own, so setting the global waker disturbs nothing else.
//!
//! It also stands in for the wiring `App` will own in Step 3: the engine's flush hook drains the
//! runtime mailbox, and the runtime waker asks the engine to enqueue a `Flush`.

use std::sync::{Arc, Mutex};
use std::thread;

use tuidom::Document;
use tuidom::headless::HeadlessRuntime;
use tuidom_ui::{
    clear_flush_waker, dyn_text, effect, flush_deferred, mount, set_flush_waker, state,
};

#[test]
fn an_off_thread_write_reaches_the_dom_on_the_next_pump() {
    let doc = Document::new().unwrap();

    // Wire the two halves App will wire in Step 3.
    doc.set_runtime_flush_hook(Arc::new(flush_deferred));
    let waker_doc = doc.clone();
    set_flush_waker(Arc::new(move || waker_doc.request_runtime_flush()));

    let value = state(0);

    // A probe recording the thread each effect run lands on, so we can prove the rerun happens on
    // the pump (event-task) side and never on the writer.
    let run_threads: Arc<Mutex<Vec<thread::ThreadId>>> = Arc::new(Mutex::new(Vec::new()));
    let probe = run_threads.clone();
    effect(move || {
        value.get();
        probe.lock().unwrap().push(thread::current().id());
    });

    let _mounted = mount(
        &doc,
        doc.root(),
        dyn_text(move || format!("{}", value.get())),
    )
    .unwrap();

    let mut rt = HeadlessRuntime::new(doc, 8, 1);
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "0");

    let runs_before = run_threads.lock().unwrap().len();
    let main_id = thread::current().id();

    // The write comes from another thread. With a waker registered and no inline span active
    // there, `schedule` defers: it marks the effects dirty into the mailbox and enqueues a
    // single `Flush`, running nothing on the writer.
    let writer = value;
    let writer_id = thread::spawn(move || {
        writer.set(7);
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
    assert_eq!(
        rt.get_cell(0, 0).unwrap().text,
        "0",
        "the DOM still shows the pre-write value until the flush is pumped"
    );

    // Pump the queued `Flush`: the hook drains the mailbox and reruns the effects on this side.
    rt.pump();
    rt.render().unwrap();
    assert_eq!(rt.get_cell(0, 0).unwrap().text, "7");

    let threads = run_threads.lock().unwrap();
    assert!(threads.len() > runs_before, "the effect reran on pump");
    assert!(
        !threads.contains(&writer_id),
        "the effect body never ran on the writing thread"
    );
    assert!(
        threads.iter().all(|id| *id == main_id),
        "every effect run was on the event-task side"
    );
    drop(threads);

    clear_flush_waker();
}
