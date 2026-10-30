use crate::event::{ApaceShellEvent, ApaceShellProxy};

use anyrender::WindowRenderer;
use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use wingy::application::ApplicationHandler;
use wingy::event::WindowEvent;
use wingy::event_loop::ActiveEventLoop;
use wingy::window::WindowId;

use wingy::{
    data_transfer::{DataTransferSendBuilder, TypeHint},
    event_loop::AsyncRequestSerial,
};

#[cfg(target_os = "macos")]
use wingy::platform::macos::ApplicationHandlerExtMacOS;

use crate::{View, WindowConfig};

pub struct ApaceApplication<Rend: WindowRenderer> {
    pub windows: HashMap<WindowId, View<Rend>>,
    pub pending_windows: Vec<WindowConfig<Rend>>,
    pub proxy: ApaceShellProxy,
    pub event_queue: Receiver<ApaceShellEvent>,
    pending_clipboard: HashMap<AsyncRequestSerial, WindowId>,
}

impl<Rend: WindowRenderer> ApaceApplication<Rend> {
    pub fn new(proxy: ApaceShellProxy, event_queue: Receiver<ApaceShellEvent>) -> Self {
        ApaceApplication {
            windows: HashMap::new(),
            pending_windows: Vec::new(),
            proxy,
            event_queue,
            pending_clipboard: HashMap::new(),
        }
    }

    pub fn add_window(&mut self, window_config: WindowConfig<Rend>) {
        self.pending_windows.push(window_config);
    }

    fn window_mut_by_doc_id(&mut self, doc_id: usize) -> Option<&mut View<Rend>> {
        self.windows.values_mut().find(|w| w.doc.id() == doc_id)
    }

    pub fn handle_apace_shell_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        event: ApaceShellEvent,
    ) {
        match event {
            ApaceShellEvent::SetClipboardText { text } => {
                let data = DataTransferSendBuilder::new(text)
                    .with_type(TypeHint::Plaintext, |text, _| Some(text.clone()))
                    .build();
                if let Err(_error) = event_loop.set_clipboard(data) {
                    #[cfg(feature = "tracing")]
                    tracing::error!("Failed to set clipboard: {_error}");
                }
            }
            ApaceShellEvent::RequestClipboardText { window_id } => {
                if let Ok(Some(id)) = event_loop.clipboard()
                    && let Ok(serial) = event_loop.fetch_data_transfer(id, &TypeHint::Plaintext)
                {
                    self.pending_clipboard.insert(serial, window_id);
                }
            }
            ApaceShellEvent::Poll { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.poll();
                };
            }
            ApaceShellEvent::CloseWindow { window_id } => {
                // Drop window before exiting event loop
                // See https://github.com/rust-windowing/winit/issues/4135
                let window = self.windows.remove(&window_id);
                drop(window);
                if self.windows.is_empty() {
                    event_loop.exit();
                }
            }
            ApaceShellEvent::ResumeReady { window_id } => {
                // The renderer fires `on_ready` after it has sent on the
                // channel, so `complete_resume` should always succeed here.
                // If a stale event survives a suspend, dropping it is safe.
                if let Some(window) = self.windows.get_mut(&window_id) {
                    let ok = window.complete_resume();
                    debug_assert!(ok, "ResumeReady received but renderer not ready");
                }
            }
            ApaceShellEvent::RequestRedraw { doc_id } => {
                // TODO: Handle multiple documents per window
                if let Some(window) = self.window_mut_by_doc_id(doc_id) {
                    window.request_redraw();
                }
            }

            #[cfg(feature = "accessibility")]
            ApaceShellEvent::Accessibility { window_id, data } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    match &*data {
                        apace_accesskit::WindowEvent::InitialTreeRequested => {
                            window.build_accessibility_tree();
                        }
                        apace_accesskit::WindowEvent::AccessibilityDeactivated => {
                            // TODO
                        }
                        apace_accesskit::WindowEvent::ActionRequested(_req) => {
                            // TODO
                        }
                    }
                }
            }
            ApaceShellEvent::Embedder(_) => {
                // Do nothing. Should be handled by embedders (if required).
            }
            ApaceShellEvent::Navigate(_opts) => {
                // Do nothing. Should be handled by embedders (if required).
            }
            ApaceShellEvent::NavigationLoad { .. } => {
                // Do nothing. Should be handled by embedders (if required).
            }
            #[cfg(target_arch = "wasm32")]
            ApaceShellEvent::ResizeSettleCheck { window_id } => {
                if let Some(window) = self.windows.get_mut(&window_id) {
                    window.apply_pending_resize_if_settled();
                }
            }
        }
    }
}

impl<Rend: WindowRenderer> ApplicationHandler for ApaceApplication<Rend> {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        // Resume existing windows
        for view in self.windows.values_mut() {
            view.resume();
        }

        // Initialise pending windows. The renderer's resume is non-blocking —
        // on native it finishes inline, on wasm32 it spawns a future that will
        // dispatch ApaceShellEvent::ResumeReady when init completes. Either way
        // we insert the view immediately so the event handler can find it.
        for window_config in self.pending_windows.drain(..) {
            let mut view = View::init(window_config, event_loop, &self.proxy);
            view.resume();
            self.windows.insert(view.window_id(), view);
        }
    }

    fn destroy_surfaces(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.windows.values_mut() {
            view.suspend();
        }
    }

    fn resumed(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // TODO
    }

    fn suspended(&mut self, _event_loop: &dyn ActiveEventLoop) {
        // TODO
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::DataTransferReceived { serial, value, .. } = &event
            && let Some(&requesting_window) = self.pending_clipboard.get(serial)
        {
            let result = value.try_as_string();
            if result
                .as_ref()
                .is_err_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
            {
                return;
            }
            self.pending_clipboard.remove(serial);
            if let Ok(text) = result
                && let Some(window) = self.windows.get_mut(&requesting_window)
            {
                window
                    .doc
                    .handle_ui_event(apace_traits::events::UiEvent::Paste(text));
                window.request_redraw();
                self.proxy.send_event(ApaceShellEvent::Poll {
                    window_id: requesting_window,
                });
            }
            return;
        }
        // Exit the app when window close is requested.
        if matches!(event, WindowEvent::CloseRequested) {
            // Drop window before exiting event loop
            // See https://github.com/rust-windowing/winit/issues/4135
            let window = self.windows.remove(&window_id);
            drop(window);
            if self.windows.is_empty() {
                event_loop.exit();
            }
            return;
        }

        if let Some(window) = self.windows.get_mut(&window_id) {
            window.handle_wingy_event(event);
        }
        self.proxy.send_event(ApaceShellEvent::Poll { window_id });
    }

    fn proxy_wake_up(&mut self, event_loop: &dyn ActiveEventLoop) {
        while let Ok(event) = self.event_queue.try_recv() {
            self.handle_apace_shell_event(event_loop, event);
        }
    }

    #[cfg(target_os = "macos")]
    fn macos_handler(&mut self) -> Option<&mut dyn ApplicationHandlerExtMacOS> {
        Some(self)
    }

    #[cfg(target_os = "ios")]
    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        for view in self.windows.values_mut() {
            if view.ios_request_redraw.get() {
                view.window.request_redraw();
            }
        }
    }
}

#[cfg(target_os = "macos")]
impl<Rend: WindowRenderer> ApplicationHandlerExtMacOS for ApaceApplication<Rend> {
    fn standard_key_binding(
        &mut self,
        _event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        action: &str,
    ) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.handle_apple_standard_keybinding(action);
            self.proxy.send_event(ApaceShellEvent::Poll { window_id });
        }
    }
}
