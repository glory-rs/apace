use super::{TextBrush, TextInputData};
use apace_traits::shell::ShellProvider;
use parley::{FontContext, LayoutContext};
use std::sync::Mutex;

#[derive(Default)]
struct Context(Mutex<Vec<(String, usize, usize)>>);
impl ShellProvider for Context {
    fn supports_ime_surrounding_text(&self) -> bool {
        true
    }
    fn set_ime_surrounding_text(&self, text: &str, cursor: usize, anchor: usize) {
        assert!(
            text.len() < 4000 && text.is_char_boundary(cursor) && text.is_char_boundary(anchor)
        );
        self.0.lock().unwrap().push((text.into(), cursor, anchor));
    }
}
#[test]
fn focused_ime_context_bounds_unicode_selection_and_excludes_passwords_and_marked_text() {
    let mut fonts = FontContext::new();
    let mut layouts = LayoutContext::<TextBrush>::new();
    let mut input = TextInputData::new(false);
    let text = "中😀e\u{301}".repeat(1000);
    input.editor.set_text(&text);
    input.editor.refresh_layout(&mut fonts, &mut layouts);
    input
        .editor
        .driver(&mut fonts, &mut layouts)
        .select_byte_range(3000, 3010);
    let provider = Context::default();
    input.sync_ime_context(&provider);
    let entries = provider.0.lock().unwrap();
    let (excerpt, cursor, anchor) = &entries[0];
    assert!(!excerpt.is_empty() && excerpt.len() < text.len());
    assert_eq!(
        &excerpt[(*cursor).min(*anchor)..(*cursor).max(*anchor)],
        &text[3000..3010]
    );
    drop(entries);
    input.set_password(true);
    input.sync_ime_context(&provider);
    assert_eq!(
        provider.0.lock().unwrap().last().unwrap(),
        &(String::new(), 0, 0)
    );
    input.set_password(false);
    input.editor.driver(&mut fonts, &mut layouts).select_all();
    input.sync_ime_context(&provider);
    assert_eq!(
        provider.0.lock().unwrap().last().unwrap(),
        &(String::new(), 0, 0)
    );
    input
        .editor
        .driver(&mut fonts, &mut layouts)
        .select_byte_range(3, 3);
    input
        .editor
        .driver(&mut fonts, &mut layouts)
        .set_compose("拼", Some((3, 3)));
    let before = provider.0.lock().unwrap().len();
    input.sync_ime_context(&provider);
    assert_eq!(
        provider.0.lock().unwrap().len(),
        before,
        "acknowledgements must not overwrite native marked text"
    );
}
