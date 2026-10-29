//! Password display indices are separate from the editable UTF-8 buffer.
//! Never shape the raw value for painting, pointer navigation or scrolling.
use super::TextBrush;
use icu_segmenter::GraphemeClusterSegmenter;
use parley::{FontContext, LayoutContext, PlainEditor};

pub(super) struct Password {
    pub editor: Box<PlainEditor<TextBrush>>,
    boundaries: Vec<usize>,
    generation: Option<parley::editing::Generation>,
    config_dirty: bool,
}

impl Password {
    pub fn new(font_size: f32) -> Self {
        Self {
            editor: Box::new(PlainEditor::new(font_size)),
            boundaries: vec![0],
            generation: None,
            config_dirty: true,
        }
    }

    pub fn invalidate_config(&mut self) {
        self.config_dirty = true;
    }

    pub fn raw_index(&self, display: usize) -> usize {
        self.boundaries[(display / '•'.len_utf8()).min(self.boundaries.len() - 1)]
    }

    pub fn display_index(&self, raw: usize, round_up: bool) -> usize {
        let index = match self.boundaries.binary_search(&raw) {
            Ok(index) => index,
            Err(index) => {
                if round_up {
                    index
                } else {
                    index.saturating_sub(1)
                }
            }
        };
        index.min(self.boundaries.len() - 1) * '•'.len_utf8()
    }

    pub fn sync_selection(
        &mut self,
        raw: &PlainEditor<TextBrush>,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
    ) {
        let selection = raw.raw_selection();
        let anchor = selection.anchor().index();
        let focus = selection.focus().index();
        let a = self.display_index(anchor, anchor >= focus);
        let f = self.display_index(focus, focus >= anchor);
        // Preserve the display editor's word/line drag anchor and h_pos when
        // synchronizing the same selection after a pointer operation.
        let current = self.editor.raw_selection();
        if current.anchor().index() != a || current.focus().index() != f {
            self.editor.driver(fonts, layouts).select_byte_range(a, f);
        }
        self.generation = Some(raw.generation());
    }

    pub fn sync(
        &mut self,
        raw: &PlainEditor<TextBrush>,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
    ) {
        if !self.config_dirty && self.generation == Some(raw.generation()) {
            return;
        }
        self.boundaries.clear();
        self.boundaries
            .extend(GraphemeClusterSegmenter::new().segment_str(raw.raw_text()));
        let count = self.boundaries.len().saturating_sub(1);
        if self.config_dirty {
            // Copy configuration, never the raw buffer or its glyph layout.
            // Width/alignment are mirrored by TextInputData's setters.
            self.editor.edit_styles().clone_from(raw.get_styles());
            self.editor.set_scale(raw.get_scale());
            self.editor.set_base_direction(raw.base_direction());
            self.editor.set_text(&"•".repeat(count));
            self.editor.refresh_layout(fonts, layouts);
            self.config_dirty = false;
        } else if self.editor.raw_text().len() != count * '•'.len_utf8() {
            self.editor.set_text(&"•".repeat(count));
            self.editor.refresh_layout(fonts, layouts);
        }
        // Compose styling uses bullet indices too; marking existing masked
        // bytes never inserts the IME's raw preedit into the display buffer.
        if let Some(compose) = raw.raw_compose() {
            let range =
                self.display_index(compose.start, false)..self.display_index(compose.end, true);
            if self.editor.raw_compose().as_ref() != Some(&range) {
                self.editor
                    .driver(fonts, layouts)
                    .set_compose_byte_range(range.start, range.end);
            }
        } else if self.editor.is_composing() {
            self.editor.driver(fonts, layouts).finish_compose();
        }
        self.sync_selection(raw, fonts, layouts);
    }

    pub fn deletion(
        &self,
        raw: &PlainEditor<TextBrush>,
        backwards: bool,
        whole: bool,
    ) -> std::ops::Range<usize> {
        let range = raw.raw_selection().text_range();
        if !range.is_empty() {
            return range;
        }
        let index = range.start;
        if whole {
            return if backwards {
                0..index
            } else {
                index..raw.raw_text().len()
            };
        }
        match self.boundaries.binary_search(&index) {
            Ok(at) if backwards => self.boundaries[at.saturating_sub(1)]..index,
            Ok(at) => index..self.boundaries[(at + 1).min(self.boundaries.len() - 1)],
            Err(at) => {
                self.boundaries[at.saturating_sub(1)]
                    ..self.boundaries[at.min(self.boundaries.len() - 1)]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::TextInputData;
    use crate::util::ACTION_MOD;
    use apace_traits::events::{ApaceImeEvent, ApaceKeyEvent, KeyState};
    use apace_traits::shell::{ClipboardError, ShellProvider};
    use keyboard_types::{Code, Key, Location, Modifiers, NamedKey};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Clipboard(Mutex<String>);
    impl ShellProvider for Clipboard {
        fn get_clipboard_text(&self) -> Result<String, ClipboardError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set_clipboard_text(&self, value: String) -> Result<(), ClipboardError> {
            *self.0.lock().unwrap() = value;
            Ok(())
        }
    }
    fn key(key: Key, modifiers: Modifiers) -> ApaceKeyEvent {
        ApaceKeyEvent {
            key,
            code: Code::Unidentified,
            modifiers,
            location: Location::Standard,
            is_auto_repeating: false,
            is_composing: false,
            state: KeyState::Pressed,
            text: None,
        }
    }
    fn input(text: &str) -> (TextInputData, FontContext, LayoutContext<TextBrush>) {
        let mut fonts = FontContext::new();
        let mut layouts = LayoutContext::new();
        let mut input = TextInputData::new(false);
        input.set_password(true);
        input.editor.set_text(text);
        input.editor.refresh_layout(&mut fonts, &mut layouts);
        input.refresh_display_config(&mut fonts, &mut layouts);
        (input, fonts, layouts)
    }
    fn press(
        input: &mut TextInputData,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
        value: NamedKey,
    ) {
        input.apply_keypress_event(
            fonts,
            layouts,
            &Clipboard::default(),
            key(Key::Named(value), Modifiers::empty()),
        );
    }

    #[test]
    fn masked_layout_has_one_identical_bullet_per_extended_grapheme_and_keeps_raw_bytes() {
        for text in [
            "",
            "Wiim123!",
            "xxxxxxxx",
            "中文中文中文中文",
            "a🙂e\u{301}👨‍👩‍👧‍👦z",
            "אבג",
        ] {
            let (input, _, _) = input(text);
            let count = GraphemeClusterSegmenter::new().segment_str(text).count() - 1;
            assert_eq!(input.editor.raw_text(), text);
            assert_eq!(input.display_editor().raw_text(), "•".repeat(count));
            assert!(input.display_editor().try_layout().is_some());
        }
        let (wide, _, _) = input("WWWWWWWW");
        let (narrow, _, _) = input("iiiiiiii");
        assert_ne!(
            wide.editor.try_layout().unwrap().full_width(),
            narrow.editor.try_layout().unwrap().full_width()
        );
        assert_eq!(
            wide.display_editor().try_layout().unwrap().full_width(),
            narrow.display_editor().try_layout().unwrap().full_width()
        );
    }

    #[test]
    fn visual_keyboard_moves_and_deletes_whole_raw_graphemes_instead_of_raw_bidi_clusters() {
        let text = "a🙂e\u{301}👨‍👩‍👧‍👦z";
        let (mut input, mut fonts, mut layouts) = input(text);
        let boundaries: Vec<_> = GraphemeClusterSegmenter::new().segment_str(text).collect();
        press(&mut input, &mut fonts, &mut layouts, NamedKey::End);
        for &index in boundaries.iter().rev().skip(1) {
            press(&mut input, &mut fonts, &mut layouts, NamedKey::ArrowLeft);
            assert_eq!(input.editor.raw_selection().focus().index(), index);
        }
        press(&mut input, &mut fonts, &mut layouts, NamedKey::End);
        press(&mut input, &mut fonts, &mut layouts, NamedKey::ArrowLeft);
        press(&mut input, &mut fonts, &mut layouts, NamedKey::Backspace);
        assert_eq!(input.editor.raw_text(), "a🙂e\u{301}z");
        assert_eq!(input.display_editor().raw_text(), "••••");
        input.apply_keypress_event(
            &mut fonts,
            &mut layouts,
            &Clipboard::default(),
            key(Key::Named(NamedKey::ArrowLeft), ACTION_MOD),
        );
        assert_eq!(input.editor.raw_selection().focus().index(), 0);
        input.set_text(&mut fonts, &mut layouts, "אבג");
        press(&mut input, &mut fonts, &mut layouts, NamedKey::End);
        press(&mut input, &mut fonts, &mut layouts, NamedKey::ArrowLeft);
        assert_eq!(input.editor.raw_selection().focus().index(), "אב".len());
    }

    #[test]
    fn pointer_coordinates_use_masked_geometry_and_return_raw_grapheme_boundaries() {
        use parley::editing::Cursor;
        use parley::layout::Affinity;
        for text in ["Wi🙂e\u{301}", "אבג", "👨‍👩‍👧‍👦ab", "ffi", "لا", "क्‍षa"]
        {
            let (mut input, mut fonts, mut layouts) = input(text);
            let boundaries: Vec<_> = GraphemeClusterSegmenter::new().segment_str(text).collect();
            for (index, raw) in boundaries.iter().enumerate() {
                let layout = input.display_editor().try_layout().unwrap();
                let rect =
                    Cursor::from_byte_index(layout, index * '•'.len_utf8(), Affinity::Downstream)
                        .geometry(layout, 1.5);
                input.point_selection(&mut fonts, &mut layouts, |driver| {
                    driver
                        .move_to_point((rect.x0 + 0.01) as f32, ((rect.y0 + rect.y1) / 2.0) as f32)
                });
                assert_eq!(
                    input.editor.raw_selection().focus().index(),
                    *raw,
                    "{text}/{index}"
                );
            }
        }
    }

    #[test]
    fn preedit_never_enters_display_text_and_hidden_ime_cursor_stays_hidden() {
        let (mut input, mut fonts, mut layouts) = input("A");
        press(&mut input, &mut fonts, &mut layouts, NamedKey::End);
        input.apply_ime_event(
            &mut fonts,
            &mut layouts,
            ApaceImeEvent::Preedit("e\u{301}🙂".into(), Some((3, 3))),
        );
        assert_eq!(input.editor.raw_text(), "Ae\u{301}🙂");
        assert_eq!(input.display_editor().raw_text(), "•••");
        assert_eq!(input.display_editor().raw_compose(), &Some(3..9));
        let area = input.ime_cursor_area();
        assert!(area.x1 > area.x0 && area.y1 > area.y0);
        input.apply_ime_event(
            &mut fonts,
            &mut layouts,
            ApaceImeEvent::Preedit("中".into(), None),
        );
        assert!(input.cursor_geometry(1.5).is_none());
        assert_eq!(input.display_editor().raw_text(), "••");
        input.apply_ime_event(&mut fonts, &mut layouts, ApaceImeEvent::Commit("国".into()));
        assert_eq!(input.editor.raw_text(), "A国");
        assert_eq!(input.display_editor().raw_text(), "••");
    }

    #[test]
    fn copy_and_cut_never_export_password_but_paste_and_apple_grapheme_deletion_work() {
        let (mut input, mut fonts, mut layouts) = input("secret🙂");
        let clipboard = Clipboard(Mutex::new("replacement中".into()));
        input.apply_keypress_event(
            &mut fonts,
            &mut layouts,
            &clipboard,
            key(Key::Character("a".into()), ACTION_MOD),
        );
        for name in ["c", "x"] {
            input.apply_keypress_event(
                &mut fonts,
                &mut layouts,
                &clipboard,
                key(Key::Character(name.into()), ACTION_MOD),
            );
            assert_eq!(clipboard.0.lock().unwrap().as_str(), "replacement中");
            assert_eq!(input.editor.raw_text(), "secret🙂");
        }
        for command in ["copy:", "cut:"] {
            input.apply_apple_standard_keybinding(&mut fonts, &mut layouts, &clipboard, command);
            assert_eq!(clipboard.0.lock().unwrap().as_str(), "replacement中");
        }
        input.apply_keypress_event(
            &mut fonts,
            &mut layouts,
            &clipboard,
            key(Key::Character("v".into()), ACTION_MOD),
        );
        assert_eq!(input.editor.raw_text(), "replacement中");
        input.apply_apple_standard_keybinding(
            &mut fonts,
            &mut layouts,
            &clipboard,
            "deleteWordBackward:",
        );
        assert_eq!(input.editor.raw_text(), "");
        assert_eq!(input.display_editor().raw_text(), "");
    }

    #[test]
    fn horizontal_scroll_uses_mask_width_and_dpi_instead_of_password_character_widths() {
        let mut offsets = Vec::new();
        for text in ["W".repeat(100), "i".repeat(100), "中".repeat(100)] {
            let (mut input, mut fonts, mut layouts) = input(&text);
            input.editor.set_scale(2.0);
            input.editor.refresh_layout(&mut fonts, &mut layouts);
            input.refresh_display_config(&mut fonts, &mut layouts);
            press(&mut input, &mut fonts, &mut layouts, NamedKey::End);
            input.clamp_scroll_offset(48., 24.);
            assert!(input.scroll_offset > 0.);
            offsets.push(input.scroll_offset);
        }
        assert_eq!(offsets[0], offsets[1]);
        assert_eq!(offsets[1], offsets[2]);
    }
}
