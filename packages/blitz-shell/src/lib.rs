#![cfg_attr(docsrs, feature(doc_cfg))]

//! Event loop, windowing and system integration.
//!
//! ## Feature flags
//!  - `default`: Enables the features listed below.
//!  - `accessibility`: Enables [`accesskit`] accessibility support.
//!  - `hot-reload`: Enables hot-reloading of Dioxus RSX.
//!  - `tracing`: Enables tracing support.

mod application;
mod convert_events;
mod event;
mod net;
mod window;

#[cfg(feature = "accessibility")]
mod accessibility;

pub use crate::application::BlitzApplication;
pub use crate::event::{BlitzShellEvent, BlitzShellProxy};
pub use crate::window::{View, WindowConfig};

#[cfg(feature = "data-uri")]
pub use crate::net::DataUriNetProvider;

#[cfg(all(
    feature = "file-dialog",
    any(
        target_os = "windows",
        target_os = "macos",
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
use blitz_traits::shell::FileDialogFilter;
use blitz_traits::shell::{ImePurpose, ShellProvider};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use winit::cursor::{Cursor, CursorIcon};
use winit::dpi::{LogicalPosition, LogicalSize};
pub use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
pub use winit::window::Window;
use winit::window::{
    ImeCapabilities, ImeEnableRequest, ImeHint, ImePurpose as WindowImePurpose, ImeRequest,
    ImeRequestData,
};

#[derive(Default)]
pub struct Config {
    pub stylesheets: Vec<String>,
    pub base_url: Option<String>,
}

/// Build an event loop for the application
pub fn create_default_event_loop() -> EventLoop {
    let mut ev_builder = EventLoop::builder();
    #[cfg(target_os = "android")]
    {
        use winit::platform::android::EventLoopBuilderExtAndroid;
        ev_builder.with_android_app(current_android_app());
    }

    let event_loop = ev_builder.build().unwrap();
    event_loop.set_control_flow(ControlFlow::Wait);

    event_loop
}

#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<android_activity::AndroidApp> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Set the current [`AndroidApp`](android_activity::AndroidApp).
pub fn set_android_app(app: android_activity::AndroidApp) {
    ANDROID_APP.set(app).unwrap()
}

#[cfg(target_os = "android")]
#[cfg_attr(docsrs, doc(cfg(target_os = "android")))]
/// Get the current [`AndroidApp`](android_activity::AndroidApp).
/// This will panic if the android activity has not been setup with [`set_android_app`].
pub fn current_android_app() -> android_activity::AndroidApp {
    ANDROID_APP.get().unwrap().clone()
}

pub struct BlitzShellProvider {
    window: Arc<dyn Window>,
    proxy: BlitzShellProxy,
    ime_password: AtomicBool,
    ime_enabled: AtomicBool,
    ime_area: Mutex<Option<[f32; 4]>>,
    #[cfg(target_os = "ios")]
    ime_context: Mutex<Option<(String, usize, usize)>>,
}
impl BlitzShellProvider {
    pub fn new(window: Arc<dyn Window>, proxy: BlitzShellProxy) -> Self {
        Self {
            window,
            proxy,
            ime_password: AtomicBool::new(false),
            ime_enabled: AtomicBool::new(false),
            ime_area: Mutex::new(None),
            #[cfg(target_os = "ios")]
            ime_context: Mutex::new(None),
        }
    }
    fn ime_data(&self) -> ImeRequestData {
        let purpose = if self.ime_password.load(Ordering::Relaxed) {
            WindowImePurpose::Password
        } else {
            WindowImePurpose::Normal
        };
        ImeRequestData::default().with_hint_and_purpose(ImeHint::NONE, purpose)
    }
}

impl ShellProvider for BlitzShellProvider {
    fn set_ime_purpose(&self, purpose: ImePurpose) {
        let password = purpose == ImePurpose::Password;
        if self.ime_password.swap(password, Ordering::Relaxed) != password
            && self.ime_enabled.load(Ordering::Relaxed)
        {
            let _ = self
                .window
                .request_ime_update(ImeRequest::Update(self.ime_data()));
        }
    }
    fn request_redraw(&self) {
        self.window.request_redraw();
    }
    fn set_cursor(&self, icon: Option<CursorIcon>) {
        match icon {
            Some(icon) => {
                self.window.set_cursor_visible(true);
                self.window.set_cursor(Cursor::Icon(icon));
            }
            None => {
                self.window.set_cursor(Cursor::Icon(CursorIcon::Default));
                self.window.set_cursor_visible(false)
            }
        }
    }
    fn set_window_title(&self, title: String) {
        self.window.set_title(&title);
    }
    fn set_ime_enabled(&self, is_enabled: bool) {
        *self.ime_area.lock().unwrap() = None;
        #[cfg(target_os = "ios")]
        {
            *self.ime_context.lock().unwrap() = None;
        }
        if is_enabled {
            let data = self.ime_data().with_cursor_area(
                LogicalPosition::new(0., 0.).into(),
                LogicalSize::new(0., 0.).into(),
            );
            #[cfg(target_os = "ios")]
            let data = data.with_surrounding_text(
                winit::window::ImeSurroundingText::new(String::new(), 0, 0).unwrap(),
            );
            let capabilities = ImeCapabilities::new()
                .with_hint_and_purpose()
                .with_cursor_area();
            #[cfg(target_os = "ios")]
            let capabilities = capabilities.with_surrounding_text();
            let result = self.window.request_ime_update(ImeRequest::Enable(
                ImeEnableRequest::new(capabilities, data).unwrap(),
            ));
            self.ime_enabled.store(result.is_ok(), Ordering::Relaxed);
        } else {
            self.ime_enabled.store(false, Ordering::Relaxed);
            let _ = self.window.request_ime_update(ImeRequest::Disable);
        }
    }
    fn set_ime_cursor_area(&self, x: f32, y: f32, width: f32, height: f32) {
        if !self.ime_enabled.load(Ordering::Relaxed) {
            return;
        }
        {
            let mut previous = self.ime_area.lock().unwrap();
            let area = [x, y, width, height];
            if *previous == Some(area) {
                return;
            }
            *previous = Some(area);
        }
        // Release the cache lock before asking the windowing system to update.
        let _ = self.window.request_ime_update(ImeRequest::Update(
            ImeRequestData::default().with_cursor_area(
                LogicalPosition::new(x, y).into(),
                LogicalSize::new(width, height).into(),
            ),
        ));
    }

    fn supports_ime_surrounding_text(&self) -> bool {
        cfg!(target_os = "ios")
    }
    fn set_ime_surrounding_text(&self, text: &str, cursor: usize, anchor: usize) {
        #[cfg(target_os = "ios")]
        {
            if !self.ime_enabled.load(Ordering::Relaxed) {
                return;
            }
            let Ok(context) =
                winit::window::ImeSurroundingText::new(text.to_owned(), cursor, anchor)
            else {
                return;
            };
            {
                let mut previous = self.ime_context.lock().unwrap();
                if previous
                    .as_ref()
                    .is_some_and(|(s, c, a)| s == text && *c == cursor && *a == anchor)
                {
                    return;
                }
                *previous = Some((text.to_owned(), cursor, anchor));
            }
            let _ = self.window.request_ime_update(ImeRequest::Update(
                ImeRequestData::default().with_surrounding_text(context),
            ));
        }
        #[cfg(not(target_os = "ios"))]
        let _ = (text, cursor, anchor);
    }

    fn request_window_close(&self) {
        self.proxy.send_event(BlitzShellEvent::CloseWindow {
            window_id: self.window.id(),
        });
    }
    fn set_window_minimized(&self, minimized: bool) {
        self.window.set_minimized(minimized);
    }
    fn set_window_maximized(&self, maximized: bool) {
        self.window.set_maximized(maximized);
    }
    fn is_window_maximized(&self) -> bool {
        self.window.is_maximized()
    }
    fn set_window_decorations(&self, decorations: bool) {
        self.window.set_decorations(decorations);
    }
    fn drag_window(&self) {
        let _ = self.window.drag_window();
    }

    #[cfg(all(
        feature = "clipboard",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn get_clipboard_text(&self) -> Result<String, blitz_traits::shell::ClipboardError> {
        let mut cb = arboard::Clipboard::new().unwrap();
        cb.get_text()
            .map_err(|_| blitz_traits::shell::ClipboardError)
    }

    #[cfg(all(
        feature = "clipboard",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn set_clipboard_text(&self, text: String) -> Result<(), blitz_traits::shell::ClipboardError> {
        let mut cb = arboard::Clipboard::new().unwrap();
        cb.set_text(text.to_owned())
            .map_err(|_| blitz_traits::shell::ClipboardError)
    }

    #[cfg(all(
        feature = "file-dialog",
        any(
            target_os = "windows",
            target_os = "macos",
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ))]
    fn open_file_dialog(
        &self,
        multiple: bool,
        filter: Option<FileDialogFilter>,
    ) -> Vec<std::path::PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        if let Some(FileDialogFilter { name, extensions }) = filter {
            dialog = dialog.add_filter(&name, &extensions);
        }
        let files = if multiple {
            dialog.pick_files()
        } else {
            dialog.pick_file().map(|file| vec![file])
        };
        files.unwrap_or_default()
    }
}
