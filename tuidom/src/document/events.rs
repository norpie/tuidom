use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use crate::animation::TransitionProperty;
use crate::document::Document;
use crate::error::{Result, TuidomError};
use crate::event::{
    EventPhase, FocusEvent, KeyEvent, Listener, ListenerHandle, ListenerKind, MouseEvent,
    PostFrameEvent, ResizeEvent, ScrollEvent, SelectionChangeEvent, TargetedEvent,
    TargetedEventKind, TransitionEndEvent, WheelEvent,
};
use crate::id::NodeId;
use crate::lock;

impl Document {
    /// Register a key press listener on a node.
    ///
    /// Key events target the focused node when one exists; otherwise they target
    /// the document root. For async work, spawn a task inside the handler.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_key_press<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut KeyEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::KeyPress,
            ListenerKind::KeyPress(Arc::new(handler)),
        )
    }

    /// Register a focus listener on a node.
    ///
    /// Focus events bubble from the node that gained focus through its ancestors.
    /// Use [`FocusEvent::relation`](crate::event::FocusEvent::relation) to distinguish
    /// self focus from descendant focus.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_focus<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut FocusEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::Focus,
            ListenerKind::Focus(Arc::new(handler)),
        )
    }

    /// Register a blur listener on a node.
    ///
    /// Blur events bubble from the node that lost focus through its ancestors.
    /// Use [`FocusEvent::relation`](crate::event::FocusEvent::relation) to distinguish
    /// self blur from descendant blur.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_blur<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut FocusEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::Blur,
            ListenerKind::Blur(Arc::new(handler)),
        )
    }

    /// Register a mouse down listener on a node.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_mouse_down<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut MouseEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::MouseDown,
            ListenerKind::MouseDown(Arc::new(handler)),
        )
    }

    /// Register a mouse up listener on a node.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_mouse_up<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut MouseEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::MouseUp,
            ListenerKind::MouseUp(Arc::new(handler)),
        )
    }

    /// Register a mouse click listener on a node.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_click<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut MouseEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::Click,
            ListenerKind::Click(Arc::new(handler)),
        )
    }

    /// Register a mouse wheel listener on a node.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_wheel<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut WheelEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::Wheel,
            ListenerKind::Wheel(Arc::new(handler)),
        )
    }

    /// Register a scroll listener on an overflow container.
    ///
    /// Fires when the node's scroll offset changes — from wheel input and imperative
    /// scrolling alike. Scroll events do not bubble.
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_scroll<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut ScrollEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::Scroll,
            ListenerKind::Scroll(Arc::new(handler)),
        )
    }

    /// Register a transition end listener on a node.
    ///
    /// Fires once a transitioned property settles on its target value, and bubbles
    /// like the DOM's `transitionend` — register on a container to observe all of
    /// its children's transitions. An interrupted transition fires no end event
    /// (its replacement fires its own), and a node removed mid-transition fires none.
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_transition_end<F>(&self, node: NodeId, handler: F) -> Result<ListenerHandle>
    where
        F: Fn(&mut TransitionEndEvent) + Send + Sync + 'static,
    {
        self.register_targeted_listener(
            node,
            TargetedEventKind::TransitionEnd,
            ListenerKind::TransitionEnd(Arc::new(handler)),
        )
    }

    /// Register a selection change listener.
    ///
    /// Selection is document state, so the event is document-level like resize: it
    /// has no target node and does not bubble. It fires only on actual change —
    /// from drag movement, clearing, and pruning after DOM mutation alike — and
    /// carries the new document-ordered selection.
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_selection_change<F>(&self, handler: F) -> ListenerHandle
    where
        F: Fn(&mut SelectionChangeEvent) + Send + Sync + 'static,
    {
        self.register_document_listener(
            &self.inner.selection_listeners,
            ListenerKind::SelectionChange(Arc::new(handler)),
        )
    }

    /// Register a terminal resize listener.
    ///
    /// Resize is document-level and does not target or bubble through nodes.
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_resize<F>(&self, handler: F) -> ListenerHandle
    where
        F: Fn(&mut ResizeEvent) + Send + Sync + 'static,
    {
        self.register_document_listener(
            &self.inner.resize_listeners,
            ListenerKind::Resize(Arc::new(handler)),
        )
    }

    /// Register a post-frame listener.
    ///
    /// Fires after each frame is rendered, carrying the frame's recorded metrics.
    /// Post-frame is document-level like resize — a frame has no target node, so
    /// the event does not target or bubble through nodes.
    ///
    /// Mutating the DOM from the handler schedules another frame, whose own
    /// post-frame event fires in turn. A handler that mutates on every event keeps
    /// the renderer permanently active; pace the mutations to let it go idle.
    ///
    /// Returns a handle that can be passed to [`remove_listener`](Self::remove_listener).
    pub fn on_post_frame<F>(&self, handler: F) -> ListenerHandle
    where
        F: Fn(&mut PostFrameEvent) + Send + Sync + 'static,
    {
        self.register_document_listener(
            &self.inner.post_frame_listeners,
            ListenerKind::PostFrame(Arc::new(handler)),
        )
    }

    fn register_targeted_listener(
        &self,
        node: NodeId,
        event_kind: TargetedEventKind,
        kind: ListenerKind,
    ) -> Result<ListenerHandle> {
        if !self.inner.nodes.contains_key(&node) {
            return Err(TuidomError::NodeNotFound { id: node });
        }

        let handle = self.next_listener_handle();
        lock::mutex(&self.inner.targeted_listeners)
            .entry((node, event_kind))
            .or_default()
            .push(Listener {
                id: handle.id,
                kind,
            });
        Ok(handle)
    }

    fn register_document_listener(
        &self,
        store: &Mutex<Vec<Listener>>,
        kind: ListenerKind,
    ) -> ListenerHandle {
        let handle = self.next_listener_handle();
        lock::mutex(store).push(Listener {
            id: handle.id,
            kind,
        });
        handle
    }

    fn next_listener_handle(&self) -> ListenerHandle {
        let id = self.inner.next_listener_id.fetch_add(1, Ordering::Relaxed);
        ListenerHandle::new(self.inner.document_id, id)
    }

    /// Remove an event listener.
    ///
    /// Returns `true` if a listener was removed, or `false` if the handle was
    /// unknown or had already been removed.
    pub fn remove_listener(&self, handle: ListenerHandle) -> bool {
        if handle.document_id != self.inner.document_id {
            return false;
        }

        let mut removed = false;

        {
            let mut targeted = lock::mutex(&self.inner.targeted_listeners);
            for listeners in targeted.values_mut() {
                let old_len = listeners.len();
                listeners.retain(|listener| listener.id != handle.id);
                removed |= listeners.len() != old_len;
            }
            targeted.retain(|_, listeners| !listeners.is_empty());
        }

        for store in [
            &self.inner.resize_listeners,
            &self.inner.post_frame_listeners,
            &self.inner.selection_listeners,
        ] {
            let mut listeners = lock::mutex(store);
            let old_len = listeners.len();
            listeners.retain(|listener| listener.id != handle.id);
            removed |= listeners.len() != old_len;
        }

        removed
    }

    /// Dispatch a key press from the current keyboard target.
    pub(crate) fn dispatch_key_press(&self, mut event: KeyEvent) {
        // With nothing focused, keys start at the active focus context rather than the
        // document root. Falling back to the root would dispatch from *outside* an open
        // modal-like context, so the context's own handlers — Escape to close, say — would
        // never see the key that was pressed while it was on screen.
        let target = self
            .focused()
            .unwrap_or_else(|| self.active_focus_context());
        self.dispatch_key_press_to(target, &mut event);
        if !event.default_prevented() && !self.apply_input_default_action(event.code) {
            self.apply_focus_default_action(event.code);
        }
    }

    pub(crate) fn dispatch_key_press_to(&self, target: NodeId, event: &mut KeyEvent) {
        self.dispatch_targeted_event(target, event, TargetedEventKind::KeyPress, |kind, event| {
            if let ListenerKind::KeyPress(handler) = kind {
                handler(event);
            }
        });
    }

    pub(crate) fn dispatch_focus_to(&self, target: NodeId) {
        let mut event = FocusEvent::new();
        self.dispatch_targeted_event(
            target,
            &mut event,
            TargetedEventKind::Focus,
            |kind, event| {
                if let ListenerKind::Focus(handler) = kind {
                    handler(event);
                }
            },
        );
    }

    pub(crate) fn dispatch_blur_to(&self, target: NodeId) {
        let mut event = FocusEvent::new();
        self.dispatch_targeted_event(
            target,
            &mut event,
            TargetedEventKind::Blur,
            |kind, event| {
                if let ListenerKind::Blur(handler) = kind {
                    handler(event);
                }
            },
        );
    }

    pub(crate) fn dispatch_mouse_down_to(&self, target: NodeId, event: &mut MouseEvent) {
        self.dispatch_targeted_event(
            target,
            event,
            TargetedEventKind::MouseDown,
            |kind, event| {
                if let ListenerKind::MouseDown(handler) = kind {
                    handler(event);
                }
            },
        );
    }

    pub(crate) fn dispatch_mouse_up_to(&self, target: NodeId, event: &mut MouseEvent) {
        self.dispatch_targeted_event(target, event, TargetedEventKind::MouseUp, |kind, event| {
            if let ListenerKind::MouseUp(handler) = kind {
                handler(event);
            }
        });
    }

    pub(crate) fn dispatch_click_to(&self, target: NodeId, event: &mut MouseEvent) {
        self.dispatch_targeted_event(target, event, TargetedEventKind::Click, |kind, event| {
            if let ListenerKind::Click(handler) = kind {
                handler(event);
            }
        });
    }

    pub(crate) fn dispatch_wheel_to(&self, target: NodeId, event: &mut WheelEvent) {
        self.dispatch_targeted_event(target, event, TargetedEventKind::Wheel, |kind, event| {
            if let ListenerKind::Wheel(handler) = kind {
                handler(event);
            }
        });
    }

    /// Dispatch a scroll event on the container whose offset changed, target phase only.
    ///
    /// Scroll does not bubble — it is high-frequency and ancestors rarely care — and it
    /// is not swallowed by inert or disabled state: like focus and blur, it reports a
    /// change the engine has already made.
    pub(crate) fn dispatch_scroll_to(&self, target: NodeId, event: &mut ScrollEvent) {
        let listeners = lock::mutex(&self.inner.targeted_listeners)
            .get(&(target, TargetedEventKind::Scroll))
            .cloned()
            .unwrap_or_default();
        if listeners.is_empty() {
            return;
        }

        event.set_dispatch_state(target, target, EventPhase::Target);
        for listener in listeners {
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let ListenerKind::Scroll(handler) = &listener.kind {
                    handler(event);
                }
            }));

            if result.is_err() {
                log::error!("event listener {} panicked", listener.id);
            }
        }
    }

    /// Dispatch a transition end event from the node whose transition finished.
    pub(crate) fn dispatch_transition_end_to(&self, target: NodeId, property: TransitionProperty) {
        let mut event = TransitionEndEvent::new(property);
        self.dispatch_targeted_event(
            target,
            &mut event,
            TargetedEventKind::TransitionEnd,
            |kind, event| {
                if let ListenerKind::TransitionEnd(handler) = kind {
                    handler(event);
                }
            },
        );
    }

    /// Build the post-frame event for the frame that was just recorded.
    ///
    /// Returns `None` when no post-frame listener is registered or no frame has
    /// been recorded yet, so frames without an audience skip the event queue
    /// entirely.
    pub(crate) fn pending_post_frame_event(&self) -> Option<PostFrameEvent> {
        if lock::mutex(&self.inner.post_frame_listeners).is_empty() {
            return None;
        }

        let snapshot = lock::mutex(&self.inner.performance).snapshot();
        Some(PostFrameEvent {
            metrics: snapshot.latest?,
            fps: snapshot.fps,
        })
    }

    pub(crate) fn dispatch_post_frame(&self, event: &mut PostFrameEvent) {
        let listeners = lock::mutex(&self.inner.post_frame_listeners).clone();
        for listener in listeners {
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let ListenerKind::PostFrame(handler) = &listener.kind {
                    handler(event);
                }
            }));

            if result.is_err() {
                log::error!("event listener {} panicked", listener.id);
            }
        }
    }

    pub(crate) fn dispatch_selection_change(&self, mut event: SelectionChangeEvent) {
        let listeners = lock::mutex(&self.inner.selection_listeners).clone();
        for listener in listeners {
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let ListenerKind::SelectionChange(handler) = &listener.kind {
                    handler(&mut event);
                }
            }));

            if result.is_err() {
                log::error!("event listener {} panicked", listener.id);
            }
        }
    }

    pub(crate) fn dispatch_resize(&self, mut event: ResizeEvent) {
        let listeners = lock::mutex(&self.inner.resize_listeners).clone();
        for listener in listeners {
            let result = catch_unwind(AssertUnwindSafe(|| {
                if let ListenerKind::Resize(handler) = &listener.kind {
                    handler(&mut event);
                }
            }));

            if result.is_err() {
                log::error!("event listener {} panicked", listener.id);
            }
        }
    }

    fn dispatch_targeted_event<E>(
        &self,
        target: NodeId,
        event: &mut E,
        event_kind: TargetedEventKind,
        invoke: impl Fn(&ListenerKind, &mut E),
    ) where
        E: TargetedEvent,
    {
        // A disabled or inert node swallows input instead of letting it bubble to an
        // interactive ancestor, matching how disabled controls behave in HTML.
        //
        // Focus, blur, and transition end are exempt: they report a change the engine
        // has already made — a node losing focus is often losing it *because* it just
        // became disabled or inert, and a transition finishes regardless of either.
        // Swallowing those would hide the change from the handler that exists to
        // observe it.
        let is_input = !matches!(
            event_kind,
            TargetedEventKind::Focus | TargetedEventKind::Blur | TargetedEventKind::TransitionEnd
        );
        if is_input && self.blocks_interaction(target) {
            return;
        }

        let path = self.event_path(target);
        if path.is_empty() {
            return;
        }

        let listener_snapshots = {
            let listeners = lock::mutex(&self.inner.targeted_listeners);
            path.iter()
                .map(|node| {
                    (
                        *node,
                        listeners
                            .get(&(*node, event_kind))
                            .cloned()
                            .unwrap_or_default(),
                    )
                })
                .collect::<Vec<_>>()
        };

        for (index, (current_target, listeners)) in listener_snapshots.into_iter().enumerate() {
            let phase = if index == 0 {
                EventPhase::Target
            } else {
                EventPhase::Bubble
            };
            event.set_dispatch_state(target, current_target, phase);

            for listener in listeners {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    invoke(&listener.kind, event);
                }));

                if result.is_err() {
                    log::error!("event listener {} panicked", listener.id);
                }
            }

            if event.propagation_stopped() {
                break;
            }
        }
    }

    pub(super) fn event_path(&self, target: NodeId) -> Vec<NodeId> {
        if !self.inner.nodes.contains_key(&target) {
            return Vec::new();
        }

        let mut path = vec![target];
        let mut current = target;
        while let Some(parent) = self.get_parent(current) {
            path.push(parent);
            current = parent;
        }
        path
    }
}
