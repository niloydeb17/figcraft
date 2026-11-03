//! Images pasted into the page. eframe's paste event carries only text, so an image copied
//! elsewhere (a screenshot, a picture from a web page) comes in here, and reaches the app as an
//! image paste event, the way the desktop shell sends one. The app then reads it as any clipboard
//! image ([`take_png`]), under the same limits. Layers copied in another design app travel as the
//! paste's HTML, which the app reads as the desktop reads the clipboard's ([`take_html`]).

use std::cell::RefCell;
use std::sync::Arc;

use egui::{Event, Key, RawInput};
use figcraft_ui_egui::FigApp;
use wasm_bindgen::JsCast as _;
use wasm_bindgen::closure::Closure;

#[derive(Default)]
struct Pasted {
    /// The image in this paste, until the next frame says whether to read it.
    file: Option<web_sys::File>,
    /// Its contents once the browser has handed them over, or why it couldn't.
    read: Option<Result<Vec<u8>, String>>,
    /// For the app to take during this frame only: a paste it ignores (a panel field has the
    /// keyboard) must not linger for a later one.
    png: Option<Vec<u8>>,
    /// The HTML of this paste when it holds copied design layers, until the next frame.
    html: Option<String>,
    /// For the app to take during this frame only, as `png`.
    html_now: Option<String>,
}

thread_local! {
    static PASTED: RefCell<Pasted> = RefCell::default();
}

/// Listen to the page's paste events, for the life of the page.
pub fn install(ctx: egui::Context) -> Result<(), wasm_bindgen::JsValue> {
    let document = web_sys::window().and_then(|w| w.document()).ok_or("the page has no document")?;
    let listener = Closure::<dyn Fn(web_sys::ClipboardEvent)>::new(move |e: web_sys::ClipboardEvent| {
        // Text is eframe's. As it does, ignore pastes aimed at something other than the app.
        let Some(data) = e.clipboard_data().filter(|_| ctx.input(|i| i.focused)) else { return };
        // Layers copied in another design app: only the HTML carries them (its text is empty).
        let html = data.get_data("text/html").ok().filter(|h| figcraft_engine::cmd::edit::design_clipboard_buffer(h).is_some());
        let image = data.get_data("text").is_ok_and(|t| t.is_empty());
        if html.is_some() || image {
            PASTED.with_borrow_mut(|p| {
                // Editable layers win over a picture of them, as on the desktop.
                if image && html.is_none() {
                    p.file = image_file(&data);
                }
                p.html = html;
            });
            ctx.request_repaint();
        }
    });
    document.add_event_listener_with_callback("paste", listener.as_ref().unchecked_ref())?;
    listener.forget();
    Ok(())
}

fn image_file(data: &web_sys::DataTransfer) -> Option<web_sys::File> {
    let files = data.files()?;
    (0..files.length()).filter_map(|i| files.get(i)).find(|f| f.type_().starts_with("image/"))
}

/// For the frame about to draw. The keys of a paste arrive with its event: Cmd/Ctrl+V's press would
/// paste our own copied layers, so it gives way to the image, as it does to text; Shift or Alt mean
/// a command of its own, that doesn't read the clipboard.
pub fn begin_frame(fig: &mut FigApp, raw_input: &mut RawInput, ctx: &egui::Context) {
    let (file, read) = PASTED.with_borrow_mut(|p| {
        p.png = None;
        // The paste's own Cmd/Ctrl+V (or its text's paste event) reads it this frame.
        p.html_now = p.html.take();
        (p.file.take(), p.read.take())
    });
    let command = |e: &Event| matches!(e, Event::Key { key: Key::V, pressed: true, modifiers, .. } if modifiers.shift || modifiers.alt);
    if let Some(file) = file.filter(|_| !raw_input.events.iter().any(command)) {
        raw_input.events.retain(|e| !matches!(e, Event::Key { key: Key::V, pressed: true, .. }));
        let ctx = ctx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let read = crate::web::read_file(&file).await;
            PASTED.with_borrow_mut(|p| p.read = Some(read));
            ctx.request_repaint();
        });
    }
    match read {
        Some(Ok(png)) => {
            PASTED.with_borrow_mut(|p| p.png = Some(png));
            // The app reads the image from `take_png`, on the desktop from the clipboard itself.
            raw_input.events.push(Event::PasteImage(Arc::default()));
        }
        Some(Err(why)) => fig.toast(why),
        None => {}
    }
}

/// The HTML pasted this frame, when it holds copied design layers.
pub fn take_html() -> Option<String> {
    PASTED.with_borrow_mut(|p| p.html_now.take())
}

/// The image pasted this frame.
pub fn take_png() -> Option<Vec<u8>> {
    PASTED.with_borrow_mut(|p| p.png.take())
}
