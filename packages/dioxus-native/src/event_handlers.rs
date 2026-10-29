use slotmap::{DefaultKey, Key, KeyData, SlotMap};
use std::cell::RefCell;
use std::rc::Rc;
use wingy::{event::WindowEvent, event_loop::ActiveEventLoop, window::WindowId};

/// The unique identifier of a window event handler. This can be used to later remove the handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WingyEventHandlerId(pub(crate) u64);

impl WingyEventHandlerId {
    /// Unregister this event handler from the window
    pub fn remove(&self) {
        let handlers: Rc<WindowEventHandlers> = dioxus_core::consume_context();
        handlers.remove(*self);
    }
}

struct WingyWindowEventHandlerInner {
    window_id: WindowId,

    #[allow(clippy::type_complexity)]
    handler: Box<dyn FnMut(&WindowEvent, &dyn ActiveEventLoop) + 'static>,
}

#[derive(Default)]
pub(crate) struct WindowEventHandlers {
    handlers: RefCell<SlotMap<DefaultKey, WingyWindowEventHandlerInner>>,
}

impl WindowEventHandlers {
    pub(crate) fn add(
        &self,
        window_id: WindowId,
        handler: impl FnMut(&WindowEvent, &dyn ActiveEventLoop) + 'static,
    ) -> WingyEventHandlerId {
        let key = self
            .handlers
            .borrow_mut()
            .insert(WingyWindowEventHandlerInner {
                window_id,
                handler: Box::new(handler),
            });
        WingyEventHandlerId(key.data().as_ffi())
    }

    pub(crate) fn remove(&self, id: WingyEventHandlerId) {
        let key = DefaultKey::from(KeyData::from_ffi(id.0));
        self.handlers.borrow_mut().remove(key);
    }

    pub fn apply_event(
        &self,
        window_id: WindowId,
        event: &WindowEvent,
        target: &dyn ActiveEventLoop,
    ) {
        for (_, handler) in self.handlers.borrow_mut().iter_mut() {
            if handler.window_id != window_id {
                continue;
            }
            (handler.handler)(event, target)
        }
    }
}
