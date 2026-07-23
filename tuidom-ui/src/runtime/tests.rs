//! Signal-runtime tests. Every test here runs without a `Document` — the reactive graph is
//! self-contained, which is the point of building it as its own layer.
//!
//! Tests share one process-wide runtime, so each builds a *disjoint* graph (its own signals
//! and effects) and observes reruns through per-graph counters rather than any global, which
//! keeps them correct under the default parallel test runner.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Scope, batch, derived, effect, state};

/// A shared counter an effect or compute closure can bump, so a test can assert how many
/// times it ran.
fn counter() -> Arc<AtomicUsize> {
    Arc::new(AtomicUsize::new(0))
}

fn count(c: &Arc<AtomicUsize>) -> usize {
    c.load(Ordering::SeqCst)
}

// ---- Step 1: storage --------------------------------------------------------------------

#[test]
fn read_and_write_round_trip() {
    let s = state(10);
    assert_eq!(s.get(), 10);
    s.set(42);
    assert_eq!(s.get(), 42);
}

#[test]
fn with_projects_without_cloning() {
    let s = state(String::from("hello"));
    let len = s.with(|v| v.len());
    assert_eq!(len, 5);
}

#[test]
fn update_mutates_in_place() {
    let s = state(vec![1, 2, 3]);
    s.update(|v| v.push(4));
    assert_eq!(s.get(), vec![1, 2, 3, 4]);
}

#[test]
fn setting_an_equal_value_does_not_rerun() {
    let s = state(1);
    let runs = counter();
    let r = runs.clone();
    effect(move || {
        s.get();
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count(&runs), 1); // ran once on creation

    s.set(1); // equal — equality skip, no rerun
    assert_eq!(count(&runs), 1);

    s.set(2); // different — reruns
    assert_eq!(count(&runs), 2);
}

// ---- Step 2: effects and tracking -------------------------------------------------------

#[test]
fn effect_reruns_on_dependency_change() {
    let s = state(0);
    let seen = state(0);
    effect(move || seen.set(s.get()));

    assert_eq!(seen.get(), 0);
    s.set(7);
    assert_eq!(seen.get(), 7);
}

#[test]
fn unrelated_writes_do_not_rerun() {
    let watched = state(0);
    let other = state(0);
    let runs = counter();
    let r = runs.clone();
    effect(move || {
        watched.get();
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count(&runs), 1);

    other.set(99); // not a dependency
    assert_eq!(count(&runs), 1);
}

#[test]
fn dependencies_are_dropped_when_a_branch_stops_reading_them() {
    let toggle = state(true);
    let a = state(1);
    let b = state(2);
    let runs = counter();
    let r = runs.clone();
    effect(move || {
        // Reads `a` only while `toggle` is true, `b` only while false.
        let _ = if toggle.get() { a.get() } else { b.get() };
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count(&runs), 1);

    toggle.set(false); // now depends on b, not a — rerun 2
    assert_eq!(count(&runs), 2);

    a.set(100); // a is no longer a dependency
    assert_eq!(count(&runs), 2);

    b.set(200); // b is now a dependency
    assert_eq!(count(&runs), 3);
}

// ---- Step 3: derived --------------------------------------------------------------------

#[test]
fn derived_memoizes_between_reads() {
    let s = state(2);
    let computes = counter();
    let c = computes.clone();
    let doubled = derived(move || {
        c.fetch_add(1, Ordering::SeqCst);
        s.get() * 2
    });

    assert_eq!(doubled.get(), 4);
    assert_eq!(doubled.get(), 4); // second read is cached
    assert_eq!(count(&computes), 1);

    s.set(5);
    assert_eq!(doubled.get(), 10); // recomputes once, lazily
    assert_eq!(count(&computes), 2);
}

#[test]
fn an_effect_reruns_through_a_derived() {
    let s = state(1);
    let doubled = derived(move || s.get() * 2);
    let seen = state(0);
    effect(move || seen.set(doubled.get()));

    assert_eq!(seen.get(), 2);
    s.set(10);
    assert_eq!(seen.get(), 20);
}

// ---- Step 4: flush, diamond, batching, operators ----------------------------------------

#[test]
fn diamond_reruns_the_effect_once_per_write() {
    let a = state(1);
    let b = derived(move || a.get() + 1);
    let c = derived(move || a.get() + 2);
    let runs = counter();
    let r = runs.clone();
    effect(move || {
        let _ = b.get() + c.get();
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count(&runs), 1);

    a.set(2); // dirties both b and c, but the effect must run exactly once
    assert_eq!(count(&runs), 2);
}

#[test]
fn a_batch_flushes_once() {
    let s = state(0);
    let runs = counter();
    let r = runs.clone();
    effect(move || {
        s.get();
        r.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(count(&runs), 1);

    batch(|| {
        s.set(1);
        s.set(2);
        s.set(3);
    });
    assert_eq!(count(&runs), 2); // one flush for the whole batch
    assert_eq!(s.get(), 3);
}

#[test]
fn add_assign_updates_and_notifies() {
    let mut count_sig = state(0);
    let seen = state(0);
    effect(move || seen.set(count_sig.get()));

    count_sig += 5;
    assert_eq!(count_sig.get(), 5);
    assert_eq!(seen.get(), 5);

    count_sig -= 2;
    assert_eq!(count_sig.get(), 3);
    assert_eq!(seen.get(), 3);
}

// ---- Step 5: scopes ---------------------------------------------------------------------

#[test]
fn dispose_frees_signals_and_read_is_inert() {
    let scope = Scope::new();
    let s = scope.run(|| state(5));
    assert_eq!(s.get(), 5);

    scope.dispose();
    // Read after dispose is inert (falls back to Default), not a panic.
    assert_eq!(s.get(), 0);
    // Write after dispose is a silent no-op.
    s.set(123);
    assert_eq!(s.get(), 0);
}

#[test]
fn cleanups_run_in_reverse_order() {
    let order = Arc::new(std::sync::Mutex::new(Vec::new()));
    let scope = Scope::new();

    for i in 0..3 {
        let o = order.clone();
        scope.on_cleanup(move || o.lock().unwrap().push(i));
    }
    scope.dispose();

    assert_eq!(*order.lock().unwrap(), vec![2, 1, 0]);
}

#[test]
fn a_child_scope_disposes_with_its_parent() {
    let parent = Scope::new();
    let child = parent.child();
    let s = child.run(|| state(1));
    assert_eq!(s.get(), 1);

    parent.dispose(); // disposing the parent disposes the child too
    assert_eq!(s.get(), 0); // child's signal is gone
}

#[tokio::test]
async fn a_spawned_task_is_aborted_on_dispose() {
    let ran_after = Arc::new(AtomicUsize::new(0));
    let scope = Scope::new();

    let flag = ran_after.clone();
    scope.spawn(async move {
        // Long enough that dispose beats it to the increment.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        flag.fetch_add(1, Ordering::SeqCst);
    });

    scope.dispose(); // aborts the task before it can increment
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(ran_after.load(Ordering::SeqCst), 0);
}

// ---- Step 6: guards ---------------------------------------------------------------------

#[test]
fn a_self_feeding_effect_terminates_instead_of_hanging() {
    // The effect writes a signal it reads. Without the flush's reentrancy and iteration
    // guards this would loop forever; the point of the test is that it returns at all.
    let s = state(0);
    effect(move || {
        let v = s.get();
        if v < 5 {
            s.set(v + 1);
        }
    });
    // The reentrancy guard means the in-effect writes queue rather than nesting, and the
    // fixpoint (v == 5) is reached without a hang.
    assert_eq!(s.get(), 5);
}

#[test]
fn an_unbounded_cycle_is_broken_by_the_iteration_guard() {
    // Unlike the fixpoint above, this effect always writes a *new* value, so it never
    // settles. The iteration backstop is the only thing standing between it and an infinite
    // flush; the test simply has to return. It runs in its own scope so the runaway effect
    // is disposed afterwards rather than left in the shared arena.
    let scope = Scope::new();
    let s = scope.run(|| state(0i64));
    scope.run(|| {
        effect(move || {
            let v = s.get();
            s.set(v + 1);
        });
    });

    // Reaching here at all is the assertion: the guard bailed instead of hanging. The value
    // landed at the guard's ceiling rather than diverging.
    assert!(s.get() > 0);
    scope.dispose();
}
