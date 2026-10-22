use blitz_traits::node_id::NodeId;
use selectors::context::QuirksMode;
use std::{io::Cursor, sync::Arc};
use style::{
    font_face::{FontFaceSourceFormat, FontFaceSourceFormatKeyword, FontStyleRange, Source},
    media_queries::MediaList,
    servo_arc::Arc as ServoArc,
    shared_lock::SharedRwLock,
    shared_lock::{Locked, SharedRwLockReadGuard},
    stylesheets::{
        AllowImportRules, CssRule, DocumentStyleSheet, ImportRule, Origin, Stylesheet,
        StylesheetInDocument, StylesheetLoader as ServoStylesheetLoader, UrlExtraData,
        import_rule::{ImportLayer, ImportSheet, ImportSupportsCondition},
    },
    values::{CssUrl, SourceLocation},
};

use crate::resource_queue::{DocumentSender, Ticket};
use blitz_traits::net::{AbortSignal, Bytes, NetHandler, NetProvider, NetReady, Request};
use blitz_traits::shell::ShellProvider;
use std::task::Poll;

use url::Url;

use crate::{document::DocumentEvent, util::ImageType};

pub(crate) fn stamped_request(url: Url, signal: Option<&AbortSignal>) -> Request {
    let mut req = Request::get(url);
    if let Some(sig) = signal {
        req = req.signal(sig.clone());
    }
    req
}

/// Carries `@font-face` descriptors from CSS parsing through to font
/// registration so `parley::Collection::register_fonts` can alias the bytes
/// under the `font-family` declared in CSS rather than whatever family name
/// the TTF's own `name` table reports.
///
/// All fields are `Option` because each descriptor is independently optional
/// at the CSS level. Missing fields fall back to the values parley reads
/// from the font's own metadata.
#[derive(Clone, Debug, Default)]
pub struct FontFaceOverrides {
    /// `font-family` descriptor (the alias the rest of the stylesheet uses).
    pub family_name: Option<String>,
    /// `font-weight` descriptor as a single CSS weight (100–900). Stylo
    /// parses this as a range; we record the lower bound, which equals the
    /// upper bound in the common single-value case.
    pub weight: Option<f32>,
    /// `font-style` descriptor mapped to fontique's `FontStyle`.
    pub style: Option<parley::fontique::FontStyle>,
}

#[derive(Clone, Debug)]
pub enum Resource {
    Image(ImageType, u32, u32, Arc<Vec<u8>>),
    #[cfg(feature = "svg")]
    Svg(ImageType, crate::node::SvgImageData),
    Css(DocumentStyleSheet),
    /// Stylesheet loaded for an `@import` rule, to be attached to the rule on the document thread
    ImportedCss(ServoArc<Locked<ImportRule>>, ServoArc<Stylesheet>),
    Font(Bytes, FontFaceOverrides),
    /// HTML fetched for an `<iframe>` element's `src`
    DocumentSrc(String),
    None,
}

pub(crate) trait Payload: Send + Sync + 'static {
    fn weight(&self, bytes: &Bytes) -> Result<usize, String> {
        Ok(bytes.len())
    }
}
pub(crate) struct ResourceHandler<T: Payload> {
    ticket: Ticket,
    doc_id: usize,
    request_id: usize,
    node_id: Option<NodeId>,
    tx: DocumentSender,
    shell_provider: Arc<dyn ShellProvider>,
    data: T,
}

impl<T: Payload> ResourceHandler<T> {
    pub(crate) fn new(
        tx: DocumentSender,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Option<Self> {
        let ticket = tx.pool.register()?;
        Some(Self::with_ticket(
            tx,
            doc_id,
            node_id,
            shell_provider,
            data,
            ticket,
        ))
    }
    fn with_ticket(
        tx: DocumentSender,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
        ticket: Ticket,
    ) -> Self {
        Self {
            request_id: ticket.id(),
            ticket,
            doc_id,
            node_id,
            tx,
            shell_provider,
            data,
        }
    }

    pub(crate) fn boxed(
        tx: DocumentSender,
        doc_id: usize,
        node_id: Option<NodeId>,
        shell_provider: Arc<dyn ShellProvider>,
        data: T,
    ) -> Option<Box<dyn NetHandler>>
    where
        ResourceHandler<T>: NetHandler,
    {
        Some(Box::new(Self::new(tx, doc_id, node_id, shell_provider, data)?) as _)
    }

    pub(crate) fn request_id(&self) -> usize {
        self.request_id
    }

    fn cost(&self, bytes: &Bytes) -> Result<usize, String> {
        if !self.ticket.live() {
            return Err("retired resource".into());
        }
        if bytes.len() > self.ticket.limits().encoded_bytes {
            self.ticket.reject();
            return Err("encoded resource budget".into());
        }
        self.data.weight(bytes)
    }
    fn prepare_bytes(&self, bytes: &Bytes) -> NetReady<'static> {
        match self.cost(bytes) {
            Ok(bytes) => Box::pin(self.ticket.wait(bytes)),
            Err(error) => Box::pin(std::future::ready(Err(error))),
        }
    }
    fn admit(&self, bytes: &Bytes) -> Result<(), String> {
        let bytes = self.cost(bytes)?;
        match self.ticket.reserve(bytes, None) {
            Poll::Ready(result) => result,
            Poll::Pending => {
                self.ticket.reject();
                Err("resource completion capacity; await prepare".into())
            }
        }
    }
    fn respond(&self, mut resolved_url: String, mut result: Result<Resource, String>) {
        if resolved_url.len() > 4096 {
            cut(&mut resolved_url, 4096);
            result = Err("resource response URL budget".into());
        }
        if let Err(error) = &mut result {
            cut(error, 512);
        }
        // CSS/SVG charge their encoded input; raster/font/HTML charge the actual
        // retained payload. Opaque AST/decoder scratch are not an RSS budget.
        let weight = match &result {
            Ok(Resource::Image(_, _, _, bytes)) => Some(bytes.len()),
            Ok(Resource::Font(bytes, overrides)) => Some(
                bytes
                    .len()
                    .saturating_add(overrides.family_name.as_ref().map_or(0, String::len)),
            ),
            Ok(Resource::DocumentSrc(html)) => Some(html.len()),
            Ok(Resource::None) | Err(_) => Some(0),
            _ => None,
        };
        if let Some(weight) = weight {
            if !matches!(self.ticket.reserve(weight, None), Poll::Ready(Ok(()))) {
                result = Err("resource completion byte budget".into());
                let _ = self.ticket.reserve(0, None);
            }
        }
        let Some(notify) = self.ticket.publish() else {
            return;
        };
        let response = ResourceLoadResponse {
            ticket: Some(self.ticket.clone()),
            request_id: self.request_id,
            node_id: self.node_id,
            resolved_url: Some(resolved_url),
            result,
        };
        let _ = self.tx.send(DocumentEvent::ResourceLoad(response));
        if notify {
            self.shell_provider.request_redraw();
        }
    }
}

fn cut(text: &mut String, limit: usize) {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    // Replace rather than truncate: large original capacities must not survive
    // in a bounded diagnostic. Request count bounds this fixed metadata too.
    if text.len() > end || text.capacity() > limit {
        *text = text[..end].to_owned();
    }
}

#[allow(unused)]
pub struct ResourceLoadResponse {
    pub request_id: usize,
    pub node_id: Option<NodeId>,
    pub resolved_url: Option<String>,
    pub result: Result<Resource, String>,
    // Rust drops struct fields in declaration order. Drop the actual resource
    // before refunding its bytes/waking producers (also on receiver close).
    pub(crate) ticket: Option<Ticket>,
}

pub struct StylesheetHandler {
    pub guard: SharedRwLock,
    pub net_provider: Arc<dyn NetProvider>,
    pub abort_signal: Option<AbortSignal>,
}
impl Payload for StylesheetHandler {}

impl NetHandler for ResourceHandler<StylesheetHandler> {
    fn prepare<'a>(&'a mut self, _url: &'a str, bytes: &'a Bytes) -> NetReady<'a> {
        self.prepare_bytes(bytes)
    }
    fn error(self: Box<Self>, resolved_url: String, error: String) {
        self.respond(resolved_url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Err(error) = self.admit(&bytes) {
            return self.respond(resolved_url, Err(error));
        }
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        let Ok(source_url) = Url::parse(&resolved_url) else {
            return self.respond(resolved_url, Err(String::from("Invalid stylesheet URL")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = Stylesheet::from_str(
            css,
            source_url.into(),
            Origin::Author,
            ServoArc::new(self.data.guard.wrap(MediaList::empty())),
            self.data.guard.clone(),
            Some(&StylesheetLoader {
                tx: self.tx.clone(),
                doc_id: self.doc_id,
                net_provider: self.data.net_provider.clone(),
                shell_provider: self.shell_provider.clone(),
                abort_signal: self.data.abort_signal.clone(),
                import_depth: 0,
            }),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        );

        self.respond(
            resolved_url,
            Ok(Resource::Css(DocumentStyleSheet(ServoArc::new(sheet)))),
        );
    }
}

/// Maximum depth of nested `@import` rules. Imports nested deeper than this
/// are refused, preventing unbounded recursion (e.g. a stylesheet whose
/// resolved `@import` URL yields another stylesheet importing in turn, with
/// the URL growing geometrically at each level).
const MAX_IMPORT_DEPTH: usize = 16;

#[derive(Clone)]
pub(crate) struct StylesheetLoader {
    pub(crate) tx: DocumentSender,
    pub(crate) doc_id: usize,
    pub(crate) net_provider: Arc<dyn NetProvider>,
    pub(crate) shell_provider: Arc<dyn ShellProvider>,
    pub(crate) abort_signal: Option<AbortSignal>,
    /// Depth of `@import` nesting for the stylesheet this loader is parsing
    /// (0 for a top-level stylesheet)
    pub(crate) import_depth: usize,
}
impl ServoStylesheetLoader for StylesheetLoader {
    fn request_stylesheet(
        &self,
        url: CssUrl,
        location: SourceLocation,
        lock: &SharedRwLock,
        media: ServoArc<Locked<MediaList>>,
        supports: Option<ImportSupportsCondition>,
        layer: ImportLayer,
    ) -> ServoArc<Locked<ImportRule>> {
        let ticket = self.tx.pool.register();
        if self.import_depth >= MAX_IMPORT_DEPTH
            || !supports.as_ref().is_none_or(|s| s.enabled)
            || ticket.is_none()
        {
            return ServoArc::new(lock.wrap(ImportRule {
                url,
                stylesheet: ImportSheet::new_refused(),
                supports,
                layer,
                source_location: location,
            }));
        }

        let import = ImportRule {
            url,
            stylesheet: ImportSheet::new_pending(),
            supports,
            layer,
            source_location: location,
        };

        let url = import.url.url().unwrap().clone();
        let import = ServoArc::new(lock.wrap(import));
        self.net_provider.fetch(
            self.doc_id,
            stamped_request(url.as_ref().clone(), self.abort_signal.as_ref()),
            Box::new(ResourceHandler::with_ticket(
                self.tx.clone(),
                self.doc_id,
                None, // node_id
                self.shell_provider.clone(),
                NestedStylesheetHandler {
                    loader: StylesheetLoader {
                        import_depth: self.import_depth + 1,
                        ..self.clone()
                    },
                    lock: lock.clone(),
                    media,
                    import_rule: import.clone(),
                    net_provider: self.net_provider.clone(),
                },
                ticket.expect("admitted stylesheet"),
            )),
        );

        import
    }
}

struct NestedStylesheetHandler {
    loader: StylesheetLoader,
    lock: SharedRwLock,
    media: ServoArc<Locked<MediaList>>,
    import_rule: ServoArc<Locked<ImportRule>>,
    net_provider: Arc<dyn NetProvider>,
}

impl Payload for NestedStylesheetHandler {}
impl NetHandler for ResourceHandler<NestedStylesheetHandler> {
    fn prepare<'a>(&'a mut self, _url: &'a str, bytes: &'a Bytes) -> NetReady<'a> {
        self.prepare_bytes(bytes)
    }
    fn error(self: Box<Self>, resolved_url: String, error: String) {
        self.respond(resolved_url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Err(error) = self.admit(&bytes) {
            return self.respond(resolved_url, Err(error));
        }
        let Ok(css) = std::str::from_utf8(&bytes) else {
            return self.respond(resolved_url, Err(String::from("Invalid UTF8")));
        };

        let Ok(source_url) = Url::parse(&resolved_url) else {
            return self.respond(resolved_url, Err(String::from("Invalid stylesheet URL")));
        };

        // NOTE(Nico): I don't *think* external stylesheets should have HTML entities escaped
        // let escaped_css = html_escape::decode_html_entities(css);

        let sheet = ServoArc::new(Stylesheet::from_str(
            css,
            UrlExtraData(ServoArc::new(source_url)),
            Origin::Author,
            self.data.media.clone(),
            self.data.lock.clone(),
            Some(&self.data.loader),
            None, // error_reporter
            QuirksMode::NoQuirks,
            AllowImportRules::Yes,
        ));

        // Fetch @font-face fonts
        fetch_font_face(
            self.tx.clone(),
            self.doc_id,
            self.node_id,
            &sheet,
            &self.data.net_provider,
            &self.shell_provider,
            &self.data.lock.read(),
            self.data.loader.abort_signal.as_ref(),
        );

        let import_rule = self.data.import_rule.clone();
        self.respond(resolved_url, Ok(Resource::ImportedCss(import_rule, sheet)))
    }
}

struct FontFaceHandler {
    format: FontFaceSourceFormatKeyword,
    overrides: FontFaceOverrides,
}
impl Payload for FontFaceHandler {
    fn weight(&self, bytes: &Bytes) -> Result<usize, String> {
        bytes
            .len()
            .checked_add(self.overrides.family_name.as_ref().map_or(0, String::len))
            .ok_or_else(|| "font size overflow".into())
    }
}
impl NetHandler for ResourceHandler<FontFaceHandler> {
    fn prepare<'a>(&'a mut self, _url: &'a str, bytes: &'a Bytes) -> NetReady<'a> {
        self.prepare_bytes(bytes)
    }
    fn error(self: Box<Self>, resolved_url: String, error: String) {
        self.respond(resolved_url, Err(error));
    }

    fn bytes(mut self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Err(error) = self.admit(&bytes) {
            return self.respond(resolved_url, Err(error));
        }
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}
impl FontFaceHandler {
    fn parse(&mut self, bytes: Bytes) -> Result<Resource, String> {
        if self.format == FontFaceSourceFormatKeyword::None && bytes.len() >= 4 {
            self.format = match &bytes.as_ref()[0..4] {
                // WOFF (v1) files begin with 0x774F4646 ('wOFF' in ascii)
                // See: <https://w3c.github.io/woff/woff1/spec/Overview.html#WOFFHeader>
                #[cfg(feature = "woff")]
                b"wOFF" => FontFaceSourceFormatKeyword::Woff,
                // WOFF2 files begin with 0x774F4632 ('wOF2' in ascii)
                // See: <https://w3c.github.io/woff/woff2/#woff20Header>
                #[cfg(feature = "woff")]
                b"wOF2" => FontFaceSourceFormatKeyword::Woff2,
                // Opentype fonts with CFF data begin with 0x4F54544F ('OTTO' in ascii)
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                b"OTTO" => FontFaceSourceFormatKeyword::Opentype,
                // Opentype fonts truetype outlines begin with 0x00010000
                // See: <https://learn.microsoft.com/en-us/typography/opentype/spec/otff#organization-of-an-opentype-font>
                &[0x00, 0x01, 0x00, 0x00] => FontFaceSourceFormatKeyword::Truetype,
                // Truetype fonts begin with 0x74727565 ('true' in ascii)
                // See: <https://developer.apple.com/fonts/TrueType-Reference-Manual/RM06/Chap6.html#ScalerTypeNote>
                b"true" => FontFaceSourceFormatKeyword::Truetype,
                _ => FontFaceSourceFormatKeyword::None,
            }
        }

        // Satisfy rustc's mutability linting with woff feature both enabled/disabled
        #[cfg(feature = "woff")]
        let mut bytes = bytes;

        match self.format {
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff1 font");

                // Use wuff crate to decompress font
                let decompressed = wuff::decompress_woff1(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff1 font");
                }
            }
            #[cfg(feature = "woff")]
            FontFaceSourceFormatKeyword::Woff2 => {
                #[cfg(feature = "tracing")]
                tracing::info!("Decompressing woff2 font");

                // Use wuff crate to decompress font
                let decompressed = wuff::decompress_woff2(&bytes).ok();

                if let Some(decompressed) = decompressed {
                    bytes = Bytes::from(decompressed);
                } else {
                    #[cfg(feature = "tracing")]
                    tracing::warn!("Failed to decompress woff2 font");
                }
            }
            FontFaceSourceFormatKeyword::None => {
                // Should this be an error?
                return Ok(Resource::None);
            }
            _ => {}
        }

        Ok(Resource::Font(bytes, std::mem::take(&mut self.overrides)))
    }
}

/// Fetch the fonts of all `@font-face` rules in `sheet`
#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_font_face(
    tx: DocumentSender,
    doc_id: usize,
    node_id: Option<NodeId>,
    sheet: &Stylesheet,
    network_provider: &Arc<dyn NetProvider>,
    shell_provider: &Arc<dyn ShellProvider>,
    read_guard: &SharedRwLockReadGuard,
    abort_signal: Option<&AbortSignal>,
) {
    fetch_font_face_rules(
        sheet.contents(read_guard).rules(read_guard).iter(),
        tx,
        doc_id,
        node_id,
        network_provider,
        shell_provider,
        read_guard,
        abort_signal,
    )
}

/// Fetch the fonts of the `@font-face` rules among `rules`
#[allow(clippy::too_many_arguments)]
pub(crate) fn fetch_font_face_rules<'a>(
    rules: impl Iterator<Item = &'a CssRule>,
    tx: DocumentSender,
    doc_id: usize,
    node_id: Option<NodeId>,
    network_provider: &Arc<dyn NetProvider>,
    shell_provider: &Arc<dyn ShellProvider>,
    read_guard: &SharedRwLockReadGuard,
    abort_signal: Option<&AbortSignal>,
) {
    rules
        .filter_map(|rule| match rule {
            CssRule::FontFace(font_face) => {
                let descriptor = &font_face.read_with(read_guard).descriptors;
                let family = descriptor.font_family.as_ref()?;
                let src = descriptor.src.as_ref()?;
                // Capture the @font-face descriptors so parley can register
                // the font under the CSS-declared family name (and weight /
                // style) rather than whatever metadata the TTF reports.
                let overrides = FontFaceOverrides {
                    family_name: Some(family.name.to_string()),
                    weight: descriptor
                        .font_weight
                        .as_ref()
                        .and_then(|range| range.0.compute().map(|w| w.value())),
                    style: descriptor.font_style.as_ref().map(stylo_to_fontique_style),
                };
                Some((src, overrides))
            }
            _ => None,
        })
        .for_each(|(source_list, overrides)| {
            // Find the first font source in the source list that specifies a font of a type
            // that we support.
            let preferred_source = source_list
                .0
                .iter()
                .filter_map(|source| match source {
                    Source::Url(url_source) => Some(url_source),
                    // TODO: support local fonts in @font-face
                    Source::Local(_) => None,
                })
                .find_map(|url_source| {
                    let mut format = match &url_source.format_hint {
                        Some(FontFaceSourceFormat::Keyword(fmt)) => *fmt,
                        Some(FontFaceSourceFormat::String(str)) => match str.as_str() {
                            "woff2" => FontFaceSourceFormatKeyword::Woff2,
                            "ttf" => FontFaceSourceFormatKeyword::Truetype,
                            "otf" => FontFaceSourceFormatKeyword::Opentype,
                            _ => FontFaceSourceFormatKeyword::None,
                        },
                        _ => FontFaceSourceFormatKeyword::None,
                    };
                    // A url with no extension, such as a `data:` url, is
                    // sniffed from its bytes once fetched.
                    if format == FontFaceSourceFormatKeyword::None
                        && let Some((_, end)) = url_source.url.as_str().rsplit_once('.')
                    {
                        format = match end {
                            "woff2" => FontFaceSourceFormatKeyword::Woff2,
                            "woff" => FontFaceSourceFormatKeyword::Woff,
                            "ttf" => FontFaceSourceFormatKeyword::Truetype,
                            "otf" => FontFaceSourceFormatKeyword::Opentype,
                            "svg" => FontFaceSourceFormatKeyword::Svg,
                            "eot" => FontFaceSourceFormatKeyword::EmbeddedOpentype,
                            _ => FontFaceSourceFormatKeyword::None,
                        }
                    }

                    if matches!(
                        format,
                        FontFaceSourceFormatKeyword::Svg
                            | FontFaceSourceFormatKeyword::EmbeddedOpentype
                    ) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping unsupported font of type {:?}", format);
                        return None;
                    }

                    #[cfg(not(feature = "woff"))]
                    if matches!(
                        format,
                        FontFaceSourceFormatKeyword::Woff | FontFaceSourceFormatKeyword::Woff2
                    ) {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping unsupported font of type {:?}", format);
                        return None;
                    }

                    // A relative url with no base url to resolve against
                    // yields None; skip the source instead of panicking
                    let Some(url) = url_source.url.url() else {
                        #[cfg(feature = "tracing")]
                        tracing::warn!("Skipping @font-face source with unresolvable url");
                        return None;
                    };
                    let url = url.as_ref().clone();
                    Some((url, format))
                });

            if let Some((url, format)) = preferred_source {
                let Some(handler) = ResourceHandler::boxed(
                    tx.clone(),
                    doc_id,
                    node_id,
                    shell_provider.clone(),
                    FontFaceHandler { format, overrides },
                ) else {
                    return;
                };
                network_provider.fetch(doc_id, stamped_request(url, abort_signal), handler);
            }
        })
}

/// Translate stylo's `@font-face` `font-style` descriptor into the fontique
/// `FontStyle` enum used by parley. Stylo encodes Italic and Oblique-with-
/// angle distinctly; CSS's bare `normal` is parsed as `Oblique(0deg, 0deg)`
/// by stylo (see the `FontStyle::parse` impl in stylo's `font_face.rs`), so
/// that pattern is treated as `Normal` here.
fn stylo_to_fontique_style(style: &FontStyleRange) -> parley::fontique::FontStyle {
    use parley::fontique::FontStyle as Fq;
    match style {
        FontStyleRange::Italic => Fq::Italic,
        FontStyleRange::Oblique(min, max) => {
            let angle = min.degrees();
            // Stylo emits `Oblique(0deg, 0deg)` for the literal CSS `normal`
            // keyword. Map that back to `Normal` so parley's font matching
            // doesn't misclassify upright fonts.
            if angle.is_none_or(|a| a == 0.0) && max.degrees().is_none_or(|a| a == 0.0) {
                Fq::Normal
            } else {
                Fq::Oblique(angle)
            }
        }
    }
}

/// Handles HTML fetched for an `<iframe>` element's `src`
pub(crate) struct DocumentSrcHandler;

impl Payload for DocumentSrcHandler {}
impl NetHandler for ResourceHandler<DocumentSrcHandler> {
    fn prepare<'a>(&'a mut self, _url: &'a str, bytes: &'a Bytes) -> NetReady<'a> {
        self.prepare_bytes(bytes)
    }
    fn error(self: Box<Self>, resolved_url: String, error: String) {
        self.respond(resolved_url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Err(error) = self.admit(&bytes) {
            return self.respond(resolved_url, Err(error));
        }
        let html = String::from_utf8_lossy(&bytes).into_owned();
        self.respond(resolved_url, Ok(Resource::DocumentSrc(html)));
    }
}

pub struct ImageHandler {
    kind: ImageType,
    limits: crate::ResourceLimits,
}
impl ImageHandler {
    pub fn with_limits(kind: ImageType, limits: crate::ResourceLimits) -> Self {
        Self { kind, limits }
    }
    pub fn new(kind: ImageType) -> Self {
        Self {
            kind,
            limits: crate::ResourceLimits::default(),
        }
    }
}

impl Payload for ImageHandler {
    fn weight(&self, bytes: &Bytes) -> Result<usize, String> {
        match image::ImageReader::new(Cursor::new(bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?
            .into_dimensions()
        {
            Ok((w, h)) => {
                let bytes = u128::from(w) * u128::from(h) * 4;
                if w > self.limits.image_dimension
                    || h > self.limits.image_dimension
                    || bytes > u128::from(self.limits.image_decoded_bytes)
                {
                    return Err("decoded image exceeds resource limits".into());
                }
                usize::try_from(bytes).map_err(|_| "image size overflow".into())
            }
            Err(error) => {
                #[cfg(feature = "svg")]
                {
                    let _ = error;
                    Ok(bytes.len())
                }
                #[cfg(not(feature = "svg"))]
                {
                    Err(error.to_string())
                }
            }
        }
    }
}
impl NetHandler for ResourceHandler<ImageHandler> {
    fn prepare<'a>(&'a mut self, _url: &'a str, bytes: &'a Bytes) -> NetReady<'a> {
        self.prepare_bytes(bytes)
    }
    fn error(self: Box<Self>, resolved_url: String, error: String) {
        self.respond(resolved_url, Err(error));
    }

    fn bytes(self: Box<Self>, resolved_url: String, bytes: Bytes) {
        if let Err(error) = self.admit(&bytes) {
            return self.respond(resolved_url, Err(error));
        }
        let result = self.data.parse(bytes);
        self.respond(resolved_url, result)
    }
}

impl ImageHandler {
    fn parse(&self, bytes: Bytes) -> Result<Resource, String> {
        // Validate dimensions before decoding or expanding RGB into RGBA.
        // A small encoded body may otherwise allocate an enormous image.
        let dimensions = image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|error| error.to_string())?
            .into_dimensions();
        if let Ok((width, height)) = dimensions {
            if width > self.limits.image_dimension
                || height > self.limits.image_dimension
                || u128::from(width) * u128::from(height) * 4
                    > u128::from(self.limits.image_decoded_bytes)
            {
                return Err("decoded image exceeds resource limits".into());
            }
        }
        let image_err = match image::ImageReader::new(Cursor::new(&bytes))
            .with_guessed_format()
            .expect("IO errors impossible with Cursor")
            .decode()
        {
            Ok(image) => {
                let width = image.width();
                let height = image.height();
                let raw_rgba8_data = image.into_rgba8().into_raw();
                return Ok(Resource::Image(
                    self.kind,
                    width,
                    height,
                    Arc::new(raw_rgba8_data),
                ));
            }
            Err(e) => e.to_string(),
        };

        #[cfg(feature = "svg")]
        let svg_err = {
            use crate::util::parse_svg_image;
            match parse_svg_image(&bytes) {
                Ok(svg) => return Ok(Resource::Svg(self.kind, svg)),
                Err(e) => e.to_string(),
            }
        };
        #[cfg(not(feature = "svg"))]
        let svg_err = "svg feature disabled";

        Err(format!(
            "Could not parse image ({} bytes): image-crate error: {image_err}; svg fallback error: {svg_err}",
            bytes.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::fontique::FontStyle as Fq;
    use style::values::specified::Angle;

    #[test]
    fn response_diagnostics_are_bounded_utf8_and_do_not_retain_large_capacities() {
        let doc = crate::BaseDocument::new(Default::default());
        let handler = ResourceHandler::new(
            doc.tx.clone(),
            doc.id(),
            None,
            doc.shell_provider.clone(),
            ImageHandler::new(ImageType::Image),
        )
        .unwrap();
        Box::new(handler).error("🙂".repeat(10000), "🙂".repeat(10000));
        let DocumentEvent::ResourceLoad(response) = doc.rx.as_ref().unwrap().try_recv().unwrap()
        else {
            panic!("resource")
        };
        let url = response.resolved_url.as_ref().unwrap();
        assert!(url.len() <= 4096 && url.capacity() <= 4096);
        let error = response.result.as_ref().unwrap_err();
        assert!(error.len() <= 512 && error.capacity() <= 512);
        assert_eq!(doc.resource_stats().requests, 1);
        drop(response);
        assert_eq!(doc.resource_stats().requests, 0);
    }

    fn oblique(min_deg: f32, max_deg: f32) -> FontStyleRange {
        FontStyleRange::Oblique(Angle::from_degrees(min_deg), Angle::from_degrees(max_deg))
    }

    #[test]
    fn italic_maps_to_italic() {
        assert_eq!(stylo_to_fontique_style(&FontStyleRange::Italic), Fq::Italic,);
    }

    #[test]
    fn oblique_zero_zero_maps_to_normal() {
        // Stylo parses bare CSS `normal` as `Oblique(0deg, 0deg)`; the
        // helper must round-trip that back to `FontStyle::Normal` so
        // parley's matching doesn't misclassify upright fonts.
        assert_eq!(stylo_to_fontique_style(&oblique(0.0, 0.0)), Fq::Normal);
    }

    #[test]
    fn oblique_single_angle_maps_to_oblique_with_min() {
        assert_eq!(
            stylo_to_fontique_style(&oblique(14.0, 14.0)),
            Fq::Oblique(Some(14.0)),
        );
    }

    #[test]
    fn oblique_range_uses_min_angle() {
        // For a range, fontique's single-angle representation takes the
        // lower bound — confirm `min` (not `max`) is what gets through.
        assert_eq!(
            stylo_to_fontique_style(&oblique(10.0, 20.0)),
            Fq::Oblique(Some(10.0)),
        );
    }
}
