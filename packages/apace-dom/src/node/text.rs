use apace_traits::{
    events::{ApaceImeEvent, ApaceKeyEvent},
    node_id::NodeId,
    shell::ShellProvider,
};
use keyboard_types::{Key, Modifiers, NamedKey};
use parley::{ContentWidths, FontContext, LayoutContext};

use super::password::Password;
use super::placeholder::Placeholder;
use crate::util::ACTION_MOD;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Parley Brush type for Apace which contains the Apace node id
pub struct TextBrush {
    /// The node id for the span
    pub id: NodeId,
}

impl TextBrush {
    pub(crate) fn from_id(id: NodeId) -> Self {
        Self { id }
    }
}

#[derive(Clone, Default)]
pub struct TextLayout {
    pub text: String,
    pub content_widths: Option<ContentWidths>,
    pub layout: parley::layout::Layout<TextBrush>,
    /// Block-axis offset (in CSS px) of the line boxes from the top of the container's content
    /// box, as applied by `align-content`.
    pub block_offset: f32,
    /// The out-of-flow positions for which some inline span with an inline box below it is a
    /// containing block (as `SPAN_*_CB` flags). The inline root claims those boxes in its place.
    /// Set by each `RunMode::PerformLayout` run of inline layout.
    pub span_cb_flags: stylo_takia::StyleFlags,
}

impl TextLayout {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn content_widths(&mut self) -> ContentWidths {
        *self
            .content_widths
            .get_or_insert_with(|| self.layout.calculate_content_widths())
    }
}

impl std::fmt::Debug for TextLayout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "TextLayout")
    }
}

// TODO: support keypress events
pub enum GeneratedTextInputEvent {
    Input,
    Select,
    PreEditChange,
    Submit,
}

pub struct TextInputData {
    /// A parley TextEditor instance
    pub editor: Box<parley::PlainEditor<TextBrush>>,
    password: Option<Password>,
    placeholder: Option<Placeholder>,
    placeholder_width: Option<f32>,
    width: Option<f32>,
    alignment: parley::layout::Alignment,
    /// Whether the input is a singleline or multiline input
    pub is_multiline: bool,
    /// The scroll offset of the text content within the input, in CSS (unscaled) pixels.
    ///
    /// For single-line inputs this is a horizontal offset; for multi-line inputs it is a
    /// vertical offset. It is kept up to date so that the caret remains visible within the
    /// input's content box.
    pub scroll_offset: f32,
}

// FIXME: Implement Clone for PlainEditor
impl Clone for TextInputData {
    fn clone(&self) -> Self {
        let mut data = TextInputData::new(self.is_multiline);
        data.set_password(self.is_password());
        data.set_placeholder(self.placeholder.as_ref().map(|hint| hint.text.as_str()));
        data
    }
}

impl TextInputData {
    /// Export only the focused editor's bounded committed excerpt. Passwords
    /// export an empty context; native marked text is never overwritten by an
    /// acknowledgement of the committed application value.
    pub(crate) fn sync_ime_context(&self, provider: &dyn ShellProvider) {
        if !provider.supports_ime_surrounding_text() || self.editor.is_composing() {
            return;
        }
        let text = self.editor.raw_text();
        let selection = self.editor.raw_selection();
        let anchor = selection.anchor().index();
        let cursor = selection.focus().index();
        let low = anchor.min(cursor);
        let high = anchor.max(cursor);
        const BUDGET: usize = 3999;
        if self.is_password() || high - low > BUDGET {
            provider.set_ime_surrounding_text("", 0, 0);
            return;
        }
        let mut start = low.saturating_sub((BUDGET - (high - low)) / 2);
        while !text.is_char_boundary(start) {
            start += 1;
        }
        let mut end = text.len().min(start + BUDGET);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        provider.set_ime_surrounding_text(&text[start..end], cursor - start, anchor - start);
    }
    pub fn new(is_multiline: bool) -> Self {
        let editor = Box::new(parley::PlainEditor::new(16.0));
        Self {
            editor,
            password: None,
            placeholder: None,
            placeholder_width: None,
            width: None,
            alignment: parley::layout::Alignment::Start,
            is_multiline,
            scroll_offset: 0.0,
        }
    }

    pub fn set_text(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        text: &str,
    ) {
        if self.editor.text() != text {
            self.editor.set_text(text);
            self.editor.driver(font_ctx, layout_ctx).refresh_layout();
            self.refresh_display(font_ctx, layout_ctx);
        }
    }

    pub fn is_password(&self) -> bool {
        self.password.is_some()
    }

    pub(crate) fn set_placeholder(&mut self, text: Option<&str>) {
        let text = text.filter(|text| text.len() <= 4096);
        if self.placeholder.as_ref().map(|hint| hint.text.as_str()) == text {
            return;
        }
        self.placeholder = text.map(Placeholder::new);
    }
    pub fn placeholder_shown(&self) -> bool {
        self.placeholder.is_some()
            && self.editor.raw_text().is_empty()
            && !self.editor.is_composing()
    }
    /// Painting only: selection, IME, copying and geometry always use the real editor.
    pub fn placeholder_layout(&self) -> Option<&parley::Layout<TextBrush>> {
        if !self.placeholder_shown() {
            return None;
        }
        self.placeholder.as_ref()?.editor.as_ref()?.try_layout()
    }
    pub(crate) fn set_placeholder_width(&mut self, width: Option<f32>) {
        if self.placeholder_width == width {
            return;
        }
        self.placeholder_width = width;
        if let Some(hint) = &mut self.placeholder {
            hint.dirty = true;
        }
    }

    pub(crate) fn set_password(&mut self, value: bool) {
        if value && self.password.is_none() {
            let mut password = Password::new(self.editor.get_font_size());
            password.editor.set_width(self.width);
            password.editor.set_alignment(self.alignment);
            self.password = Some(password);
        } else if !value {
            self.password = None;
        }
    }

    pub(crate) fn set_width(&mut self, width: Option<f32>) {
        self.set_placeholder_width(width);
        self.width = width;
        self.editor.set_width(width);
        if let Some(password) = &mut self.password {
            password.editor.set_width(width);
        }
    }

    pub(crate) fn set_alignment(&mut self, alignment: parley::layout::Alignment) {
        if self.alignment != alignment
            && let Some(hint) = &mut self.placeholder
        {
            hint.dirty = true;
        }
        self.alignment = alignment;
        self.editor.set_alignment(alignment);
        if let Some(password) = &mut self.password {
            password.editor.set_alignment(alignment);
        }
    }

    /// The only layout used for visual geometry. Editable bytes/IME offsets
    /// deliberately remain available through the raw editor above.
    pub fn display_editor(&self) -> &parley::PlainEditor<TextBrush> {
        self.password
            .as_ref()
            .map_or(&self.editor, |password| &password.editor)
    }

    pub fn cursor_geometry(&self, size: f32) -> Option<parley::BoundingBox> {
        self.editor
            .is_cursor_visible()
            .then(|| {
                let display = self.display_editor();
                display
                    .try_layout()
                    .map(|layout| display.raw_selection().focus().geometry(layout, size))
            })
            .flatten()
    }

    pub fn ime_cursor_area(&self) -> parley::BoundingBox {
        let display = self.display_editor();
        if let (Some(password), Some(compose), Some(layout)) = (
            &self.password,
            self.editor.raw_compose(),
            display.try_layout(),
        ) {
            use parley::editing::{Cursor, Selection};
            use parley::layout::Affinity;
            let selection = Selection::new(
                Cursor::from_byte_index(
                    layout,
                    password.display_index(compose.start, false),
                    Affinity::Downstream,
                ),
                Cursor::from_byte_index(
                    layout,
                    password.display_index(compose.end, true),
                    Affinity::Upstream,
                ),
            );
            let mut bounds = display.ime_cursor_area();
            for (rect, _) in selection.geometry(layout) {
                bounds.x0 = bounds.x0.min(rect.x0);
                bounds.y0 = bounds.y0.min(rect.y0);
                bounds.x1 = bounds.x1.max(rect.x1);
                bounds.y1 = bounds.y1.max(rect.y1);
            }
            bounds
        } else {
            display.ime_cursor_area()
        }
    }

    pub(crate) fn refresh_display(
        &mut self,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
    ) {
        if let Some(password) = &mut self.password {
            password.sync(&self.editor, fonts, layouts);
        }
        if self.editor.raw_text().is_empty() {
            self.scroll_offset = 0.;
        }
        if let Some(hint) = &mut self.placeholder {
            hint.sync(
                &self.editor,
                self.placeholder_width,
                self.alignment,
                self.is_multiline,
                fonts,
                layouts,
            );
        }
    }

    pub(crate) fn refresh_display_config(
        &mut self,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
    ) {
        if let Some(password) = &mut self.password {
            password.invalidate_config();
        }
        if let Some(hint) = &mut self.placeholder {
            hint.dirty = true;
        }
        self.refresh_display(fonts, layouts);
    }

    /// Perform selection in display coordinates, then map the resulting UTF-8
    /// bullet indices back to raw extended-grapheme boundaries.
    pub(crate) fn point_selection(
        &mut self,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
        apply: impl FnOnce(&mut parley::PlainEditorDriver<TextBrush>),
    ) {
        self.refresh_display(fonts, layouts);
        if let Some(password) = &mut self.password {
            apply(&mut password.editor.driver(fonts, layouts));
            let selection = password.editor.raw_selection();
            let anchor = password.raw_index(selection.anchor().index());
            let focus = password.raw_index(selection.focus().index());
            self.editor
                .driver(fonts, layouts)
                .select_byte_range(anchor, focus);
            password.sync_selection(&self.editor, fonts, layouts);
        } else {
            apply(&mut self.editor.driver(fonts, layouts));
        }
    }

    fn password_delete(
        &mut self,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
        backwards: bool,
        whole: bool,
    ) {
        self.refresh_display(fonts, layouts);
        let range =
            self.password
                .as_ref()
                .expect("password")
                .deletion(&self.editor, backwards, whole);
        let mut driver = self.editor.driver(fonts, layouts);
        driver.select_byte_range(range.start, range.end);
        driver.delete_selection();
        self.refresh_display(fonts, layouts);
    }

    /// Recompute [`Self::scroll_offset`] so that the caret stays visible within the input's
    /// content box.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn clamp_scroll_offset(&mut self, content_box_width: f32, content_box_height: f32) {
        let Some(layout) = self.display_editor().try_layout() else {
            return;
        };
        // Parley lays out at the editor's scale, so its geometry is in scaled (device) pixels.
        // We convert into CSS (unscaled) pixels to match `scroll_offset` and the content box.
        let scale = layout.scale();

        // The caret geometry relative to the start of the text content.
        let Some(caret) = self.cursor_geometry(1.5) else {
            return;
        };

        // Caret bounds and content/viewport extents along the scrolling axis (CSS pixels).
        let (caret_start, caret_end, content, viewport) = if self.is_multiline {
            (
                caret.y0 as f32 / scale,
                caret.y1 as f32 / scale,
                layout.height() / scale,
                content_box_height,
            )
        } else {
            (
                caret.x0 as f32 / scale,
                caret.x1 as f32 / scale,
                layout.full_width() / scale,
                content_box_width,
            )
        };

        let mut offset = self.scroll_offset;

        // Scroll so that both edges of the caret are within the visible region.
        if caret_end > offset + viewport {
            offset = caret_end - viewport;
        }
        if caret_start < offset {
            offset = caret_start;
        }

        // Never scroll past the content, and never scroll into negative space. The content
        // extent includes the caret so that a caret at the very end remains fully visible
        // (its rendered width extends slightly past the text).
        let max_offset = (content.max(caret_end) - viewport).max(0.0);
        self.scroll_offset = offset.clamp(0.0, max_offset);
    }

    /// The maximum valid value of [`Self::scroll_offset`] (in CSS pixels) given the input's
    /// content box, i.e. the extent by which the text content overflows the content box along
    /// the input's scroll axis.
    ///
    /// `content_box_width` and `content_box_height` are the dimensions of the input's content
    /// box in CSS (unscaled) pixels.
    pub fn max_scroll_offset(&self, content_box_width: f32, content_box_height: f32) -> f32 {
        let Some(layout) = self.display_editor().try_layout() else {
            return 0.0;
        };
        let scale = layout.scale();
        let (content, viewport) = if self.is_multiline {
            (layout.height() / scale, content_box_height)
        } else {
            (layout.full_width() / scale, content_box_width)
        };
        (content - viewport).max(0.0)
    }

    /// Scroll the input's text content by `delta` CSS pixels along its scroll axis (horizontal
    /// for single-line inputs, vertical for multi-line inputs), clamping to the scrollable
    /// range.
    ///
    /// Returns the portion of `delta` that could not be consumed (because the input was already
    /// scrolled to its limit), so the caller can bubble it up to an ancestor scroller.
    pub fn scroll_by(
        &mut self,
        delta: f32,
        content_box_width: f32,
        content_box_height: f32,
    ) -> f32 {
        let max_offset = self.max_scroll_offset(content_box_width, content_box_height);
        if max_offset <= 0.0 {
            return delta;
        }

        // Match the sign convention used for block scrolling: a positive delta decreases the
        // scroll offset.
        let new_offset = (self.scroll_offset - delta).clamp(0.0, max_offset);
        let consumed = self.scroll_offset - new_offset;
        self.scroll_offset = new_offset;
        delta - consumed
    }

    pub(crate) fn apply_keypress_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        mut event: ApaceKeyEvent,
    ) -> Option<GeneratedTextInputEvent> {
        if self.is_password() && event.state.is_pressed() {
            let action = event.modifiers.contains(ACTION_MOD);
            if action
                && matches!(&event.key, Key::Character(c) if c.eq_ignore_ascii_case("c") || c.eq_ignore_ascii_case("x"))
            {
                return None;
            }
            if matches!(
                event.key,
                Key::Named(NamedKey::Delete | NamedKey::Backspace)
            ) {
                self.password_delete(
                    font_ctx,
                    layout_ctx,
                    event.key == Key::Named(NamedKey::Backspace),
                    action,
                );
                return Some(GeneratedTextInputEvent::Input);
            }
            let navigation = matches!(
                event.key,
                Key::Named(
                    NamedKey::ArrowLeft
                        | NamedKey::ArrowRight
                        | NamedKey::ArrowUp
                        | NamedKey::ArrowDown
                        | NamedKey::Home
                        | NamedKey::End
                )
            ) || action
                && matches!(&event.key, Key::Character(c) if c.eq_ignore_ascii_case("a"));
            if navigation {
                if action && event.key == Key::Named(NamedKey::ArrowLeft) {
                    event.key = Key::Named(NamedKey::Home);
                } else if action && event.key == Key::Named(NamedKey::ArrowRight) {
                    event.key = Key::Named(NamedKey::End);
                }
                self.refresh_display(font_ctx, layout_ctx);
                let mut password = self.password.take().expect("password");
                std::mem::swap(&mut self.editor, &mut password.editor);
                let result =
                    self.apply_plain_keypress_event(font_ctx, layout_ctx, shell_provider, event);
                std::mem::swap(&mut self.editor, &mut password.editor);
                let selection = password.editor.raw_selection();
                let a = password.raw_index(selection.anchor().index());
                let f = password.raw_index(selection.focus().index());
                self.editor
                    .driver(font_ctx, layout_ctx)
                    .select_byte_range(a, f);
                password.sync_selection(&self.editor, font_ctx, layout_ctx);
                self.password = Some(password);
                return result;
            }
        }
        let result = self.apply_plain_keypress_event(font_ctx, layout_ctx, shell_provider, event);
        self.refresh_display(font_ctx, layout_ctx);
        result
    }

    fn apply_plain_keypress_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        event: ApaceKeyEvent,
    ) -> Option<GeneratedTextInputEvent> {
        // Do nothing if it is a keyup event
        if !event.state.is_pressed() {
            return None;
        }

        let mods = event.modifiers;
        let shift = mods.contains(Modifiers::SHIFT);
        let action_mod = mods.contains(ACTION_MOD);

        let is_multiline = self.is_multiline;
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        match event.key {
            Key::Character(c) if action_mod && matches!(c.as_str(), "c" | "x" | "v") => {
                match c.to_lowercase().as_str() {
                    "c" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                        }
                    }
                    "x" => {
                        if let Some(text) = driver.editor.selected_text() {
                            let _ = shell_provider.set_clipboard_text(text.to_owned());
                            driver.delete_selection()
                        }
                    }
                    "v" => {
                        let text = shell_provider.get_clipboard_text().unwrap_or_default();
                        driver.insert_or_replace_selection(&text)
                    }
                    _ => unreachable!(),
                }

                return Some(GeneratedTextInputEvent::Input);
            }
            Key::Character(c) if action_mod && matches!(c.to_lowercase().as_str(), "a") => {
                if shift {
                    driver.collapse_selection()
                } else {
                    driver.select_all()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if action_mod {
                    if shift {
                        driver.select_word_left()
                    } else {
                        driver.move_word_left()
                    }
                } else if shift {
                    driver.select_left()
                } else {
                    driver.move_left()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowRight) => {
                if action_mod {
                    if shift {
                        driver.select_word_right()
                    } else {
                        driver.move_word_right()
                    }
                } else if shift {
                    driver.select_right()
                } else {
                    driver.move_right()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowUp) => {
                if shift {
                    driver.select_up()
                } else {
                    driver.move_up()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::ArrowDown) => {
                if shift {
                    driver.select_down()
                } else {
                    driver.move_down()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::Home) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_start()
                    } else {
                        driver.move_to_text_start()
                    }
                } else if shift {
                    driver.select_to_line_start()
                } else {
                    driver.move_to_line_start()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::End) => {
                if action_mod {
                    if shift {
                        driver.select_to_text_end()
                    } else {
                        driver.move_to_text_end()
                    }
                } else if shift {
                    driver.select_to_line_end()
                } else {
                    driver.move_to_line_end()
                }
                return Some(GeneratedTextInputEvent::Select);
            }
            Key::Named(NamedKey::Delete) => {
                if action_mod {
                    driver.delete_word()
                } else {
                    driver.delete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            // On macOS this is handled by the apple standard keybindings
            #[cfg(not(target_os = "macos"))]
            Key::Named(NamedKey::Backspace) => {
                if action_mod {
                    driver.backdelete_word()
                } else {
                    driver.backdelete()
                }
                return Some(GeneratedTextInputEvent::Input);
            }

            Key::Character(c) if c == "\n" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Named(NamedKey::Enter) => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            Key::Character(s)
                if !mods.contains(Modifiers::CONTROL) && !mods.contains(Modifiers::META) =>
            {
                driver.insert_or_replace_selection(&s);
                return Some(GeneratedTextInputEvent::Input);
            }
            _ => {}
        };

        None
    }

    pub(crate) fn apply_apple_standard_keybinding(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        if self.is_password() {
            if matches!(command, "copy:" | "cut:") {
                return None;
            }
            let deletion = match command {
                "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
                    Some((true, false))
                }
                "deleteForward:" => Some((false, false)),
                "deleteWordBackward:"
                | "deleteToBeginningOfLine:"
                | "deleteToBeginningOfParagraph:" => Some((true, true)),
                "deleteWordForward:" | "deleteToEndOfLine:" | "deleteToEndOfParagraph:" => {
                    Some((false, true))
                }
                _ => None,
            };
            if let Some((backwards, whole)) = deletion {
                self.password_delete(font_ctx, layout_ctx, backwards, whole);
                return Some(GeneratedTextInputEvent::Input);
            }
            if command.starts_with("move") || command.starts_with("select") {
                if command.contains("Word") {
                    self.point_selection(font_ctx, layout_ctx, |driver| {
                        if command.starts_with("select") {
                            driver.select_all();
                        } else if command.contains("Backward") || command.contains("Left") {
                            if command.contains("ModifySelection") {
                                driver.select_to_text_start();
                            } else {
                                driver.move_to_text_start();
                            }
                        } else if command.contains("ModifySelection") {
                            driver.select_to_text_end();
                        } else {
                            driver.move_to_text_end();
                        }
                    });
                    return Some(GeneratedTextInputEvent::Select);
                }
                self.refresh_display(font_ctx, layout_ctx);
                let mut password = self.password.take().expect("password");
                std::mem::swap(&mut self.editor, &mut password.editor);
                let result = self.apply_plain_apple_standard_keybinding(
                    font_ctx,
                    layout_ctx,
                    shell_provider,
                    command,
                );
                std::mem::swap(&mut self.editor, &mut password.editor);
                let selection = password.editor.raw_selection();
                let a = password.raw_index(selection.anchor().index());
                let f = password.raw_index(selection.focus().index());
                self.editor
                    .driver(font_ctx, layout_ctx)
                    .select_byte_range(a, f);
                password.sync_selection(&self.editor, font_ctx, layout_ctx);
                self.password = Some(password);
                return result;
            }
        }
        let result = self.apply_plain_apple_standard_keybinding(
            font_ctx,
            layout_ctx,
            shell_provider,
            command,
        );
        self.refresh_display(font_ctx, layout_ctx);
        result
    }

    fn apply_plain_apple_standard_keybinding(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        shell_provider: &dyn ShellProvider,
        command: &str,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);
        let is_multiline = self.is_multiline;

        match command {
            // Inserting Content

            // Inserts a backtab character.
            "insertBacktab:" => {}
            // Inserts a container break, such as a new page break.
            "insertContainerBreak:" => {}
            // Inserts a double quotation mark without substituting a curly quotation mark.
            "insertDoubleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("\"");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a line break character.
            "insertLineBreak:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a newline character.
            "insertNewline:" => {
                if is_multiline {
                    driver.insert_or_replace_selection("\n");
                    return Some(GeneratedTextInputEvent::Input);
                } else {
                    return Some(GeneratedTextInputEvent::Submit);
                }
            }
            // Inserts a newline character without invoking the field editor’s normal handling to end editing.
            "insertNewlineIgnoringFieldEditor:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a paragraph separator.
            "insertParagraphSeparator:" => {
                driver.insert_or_replace_selection("\n");
                return Some(GeneratedTextInputEvent::Input);
            }
            "insertSingleQuoteIgnoringSubstitution:" => {
                driver.insert_or_replace_selection("'");
                return Some(GeneratedTextInputEvent::Input);
            }
            // Inserts a tab character.
            "insertTab:" | "insertTabIgnoringFieldEditor:" => {
                // Ignore for now seeing as parley has poor support for laying out tabs
            }
            // Inserts the text you specify.
            "insertText:" => {}

            // Deleting Content

            // Deletes content moving backward from the current insertion point.
            // TODO: handle deleteBackwardByDecomposingPreviousCharacter separately
            "deleteBackward:" | "deleteBackwardByDecomposingPreviousCharacter:" => {
                driver.backdelete();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteForward:" => {
                driver.delete();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current line.
            "deleteToBeginningOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the beginning of the current paragraph.
            "deleteToEndOfLine:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            "deleteToBeginningOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_start();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }

            // Deletes content from the insertion point to the end of the current line.
            "deleteToEndOfParagraph:" => {
                if driver.editor.raw_selection().is_collapsed() {
                    driver.select_to_hard_line_end();
                }
                driver.delete_selection();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes content from the insertion point to the end of the current paragraph.
            "deleteWordBackward:" => {
                driver.backdelete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the word preceding the current insertion point.
            "deleteWordForward:" => {
                driver.delete_word();
                return Some(GeneratedTextInputEvent::Input);
            }
            // Deletes the current selection, placing it in a temporary buffer, such as the Clipboard.
            "yank:" => {
                if let Some(text) = driver.editor.selected_text() {
                    let _ = shell_provider.set_clipboard_text(text.to_owned());
                    driver.delete_selection();
                    return Some(GeneratedTextInputEvent::Input);
                }
            }

            // Moving the Insertion Pointer

            // Moves the insertion pointer backward in the current content.
            "moveBackward:" => {
                driver.move_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moves the insertion pointer down in the current content.
            "moveDown:" => {
                driver.move_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer forward in the current content.
            "moveForward:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            } // TODO: Bidi-aware

            // Moves the insertion pointer left in the current content.
            "moveLeft:" => {
                driver.move_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer right in the current content.
            "moveRight:" => {
                driver.move_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Moves the insertion pointer up in the current content.
            "moveUp:" => {
                driver.move_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Modifying the Selection

            // Extends the selection to include the content before the current selection.
            "moveBackwardAndModifySelection:" => {
                driver.select_left(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content below the current selection.
            "moveDownAndModifySelection:" => {
                driver.select_down();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content after the current selection.
            "moveForwardAndModifySelection:" => {
                driver.select_right(); // TODO: Bidi-aware
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the left of the current selection.
            "moveLeftAndModifySelection:" => {
                driver.select_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content to the right of the current selection.
            "moveRightAndModifySelection:" => {
                driver.select_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            // Extends the selection to include the content above the current selection.
            "moveUpAndModifySelection:" => {
                driver.select_up();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Changing the Selection
            "selectAll:" => {
                driver.select_all();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectLine:" => {
                driver.move_to_line_start();
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectParagraph:" => {
                driver.move_to_hard_line_start();
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "selectWord:" => {
                // TODO
            }

            // Moving the Selection in Documents
            "moveToBeginningOfDocument:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfDocumentAndModifySelection:" => {
                driver.select_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocument:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfDocumentAndModifySelection:" => {
                driver.move_to_text_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Paragraphs
            "moveParagraphBackwardAndModifySelection:" => {}
            "moveParagraphForwardAndModifySelection:" => {}
            "moveToBeginningOfParagraph:" => {
                driver.move_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraph:" => {
                driver.move_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfParagraphAndModifySelection:" => {
                driver.select_to_hard_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection in Lines of Text
            "moveToBeginningOfLine:" => {
                driver.move_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToBeginningOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLine:" => {
                driver.move_to_text_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToLeftEndOfLineAndModifySelection:" => {
                driver.select_to_line_start();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLine:" => {
                driver.move_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveToRightEndOfLineAndModifySelection:" => {
                driver.select_to_line_end();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Moving the Selection by Word Boundaries
            "moveWordBackward:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordBackwardAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForward:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordForwardAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeft:" => {
                driver.move_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordLeftAndModifySelection:" => {
                driver.select_word_left();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRight:" => {
                driver.move_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }
            "moveWordRightAndModifySelection:" => {
                driver.select_word_right();
                return Some(GeneratedTextInputEvent::Select);
            }

            // Scrolling Content

            // Scrolls the content down by a page.
            "scrollPageDown:" => {}
            // Scrolls the content up by a page.
            "scrollPageUp:" => {}
            // Scrolls the content down by a line.
            "scrollLineDown:" => {}
            // Scrolls the content up by a line.
            "scrollLineUp:" => {}
            // Scrolls the content to the beginning of the document.
            "scrollToBeginningOfDocument:" => {}
            // Scrolls the content to the end of the document.
            "scrollToEndOfDocument:" => {}
            // Moves the visible content region down by a page.
            "pageDown:" => {}
            // Moves the visible content region up by a page.
            "pageUp:" => {}
            // Moves the visible content region down by a page, and extends the current selection.
            "pageDownAndModifySelection:" => {}
            // Moves the visible content region up by a page, and extends the current selection.
            "pageUpAndModifySelection:" => {}
            // Moves the visible content region so the current selection is visually centered.
            "centerSelectionInVisibleArea:" => {}

            // Transposing Elements

            // Transposes the content around the current selection.
            "transpose:" => {}
            // Transposes the words around the current selection.
            "transposeWords:" => {}

            // Indenting Content
            // Indents the content at the current selection.
            "indent:" => {}

            // Canceling Operations
            // Cancels the current operation.
            "cancelOperation:" => {}

            // Supporting QuickLook
            // Invokes QuickLook to preview the current selection.
            "quickLookPreviewItems:" => {}

            // Supporting Writing Directions
            "makeBaseWritingDirectionLeftToRight:" => {}
            "makeBaseWritingDirectionNatural:" => {}
            "makeBaseWritingDirectionRightToLeft:" => {}
            "makeTextWritingDirectionLeftToRight:" => {}
            "makeTextWritingDirectionNatural:" => {}
            "makeTextWritingDirectionRightToLeft:" => {}

            // Changing Capitalization
            "capitalizeWord:" => {}
            "changeCaseOfLetter:" => {}
            "lowercaseWord:" => {}
            "uppercaseWord:" => {}

            // Supporting Marked Selections
            "setMark:" => {}
            "selectToMark:" => {}
            "deleteToMark:" => {}
            "swapWithMark:" => {}

            // Supporting Autocomplete
            "complete:" => {}

            // Instance Methods
            "showContextMenuForSelection:" => {}

            // Unknown command
            _ => {}
        };

        None
    }

    pub(crate) fn apply_ime_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        event: ApaceImeEvent,
    ) -> Option<GeneratedTextInputEvent> {
        let result = self.apply_plain_ime_event(font_ctx, layout_ctx, event);
        self.refresh_display(font_ctx, layout_ctx);
        result
    }

    fn apply_plain_ime_event(
        &mut self,
        font_ctx: &mut FontContext,
        layout_ctx: &mut LayoutContext<TextBrush>,
        event: ApaceImeEvent,
    ) -> Option<GeneratedTextInputEvent> {
        let editor = &mut self.editor;
        let mut driver = editor.driver(font_ctx, layout_ctx);

        match event {
            ApaceImeEvent::Enabled => {
                // Do nothing
                None
            }
            ApaceImeEvent::Disabled => {
                driver.clear_compose();
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            ApaceImeEvent::Commit(text) => {
                driver.clear_compose();
                driver.insert_or_replace_selection(&text);
                Some(GeneratedTextInputEvent::Input)
            }
            ApaceImeEvent::Preedit(text, cursor) => {
                if text.is_empty() {
                    driver.clear_compose();
                } else {
                    driver.set_compose(&text, cursor);
                }
                Some(GeneratedTextInputEvent::PreEditChange)
            }
            ApaceImeEvent::DeleteSurrounding {
                before_bytes,
                after_bytes,
            } => {
                let _ = before_bytes;
                let _ = after_bytes;
                // TODO
                None
            }
        }
    }
}

#[cfg(test)]
#[path = "ime_context_tests.rs"]
mod ime_context_tests;
