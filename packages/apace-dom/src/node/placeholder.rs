//! One bounded presentation hint. It never becomes the editor's editable bytes.
use super::TextBrush;
use parley::{FontContext, LayoutContext, PlainEditor};

pub(super) struct Placeholder {
    pub text: String,
    pub editor: Option<Box<PlainEditor<TextBrush>>>,
    pub dirty: bool,
}
impl Placeholder {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.into(),
            editor: None,
            dirty: true,
        }
    }
    pub fn sync(
        &mut self,
        raw: &PlainEditor<TextBrush>,
        width: Option<f32>,
        alignment: parley::layout::Alignment,
        multiline: bool,
        fonts: &mut FontContext,
        layouts: &mut LayoutContext<TextBrush>,
    ) {
        if self.text.is_empty() || !raw.raw_text().is_empty() || raw.is_composing() {
            // Filled/composing editors retain only their current hint string,
            // not a shaped layout for a previously visible hint.
            self.editor = None;
            self.dirty = true;
            return;
        }
        let editor = self
            .editor
            .get_or_insert_with(|| Box::new(PlainEditor::new(raw.get_font_size())));
        if !self.dirty {
            return;
        }
        editor.edit_styles().clone_from(raw.get_styles());
        editor
            .edit_styles()
            .insert(parley::style::StyleProperty::WhiteSpaceCollapse(
                parley::WhiteSpaceCollapse::Preserve,
            ));
        if !multiline {
            editor
                .edit_styles()
                .insert(parley::style::StyleProperty::TextWrapMode(
                    parley::style::TextWrapMode::NoWrap,
                ));
        }
        editor.set_scale(raw.get_scale());
        editor.set_base_direction(raw.base_direction());
        editor.set_alignment(alignment);
        editor.set_width(width);
        editor.set_text(&self.text);
        editor.refresh_layout(fonts, layouts);
        self.dirty = false;
    }
}
