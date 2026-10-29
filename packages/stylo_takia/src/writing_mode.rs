//! Mapping of physical styles into the inline/block axes of a writing mode (`writing-mode` feature).
//!
//! Takia is a horizontal-tb engine: `width`/x is the inline axis and `height`/+y the block axis.
//! When a box has a vertical `writing-mode`, Apace runs Takia in that box's axes: Takia's x axis
//! is the physical y axis (top to bottom) and Takia's y axis is the writing mode's block axis
//! (right-to-left for `vertical-rl`/`sideways-rl`, left-to-right for `vertical-lr`/`sideways-lr`).
//! [`WritingModeExt`] maps physical values into those axes. Without the `writing-mode` feature
//! every writing mode is horizontal and the methods reduce to the identity.
//!
//! Takia side -> physical side in a vertical writing mode:
//!
//! | Takia        | physical                                         |
//! |--------------|--------------------------------------------------|
//! | `left`       | top                                              |
//! | `right`      | bottom                                           |
//! | `top`        | block-start side (`right` for `*-rl`, `left` for `*-lr`) |
//! | `bottom`     | block-end side                                   |
//!
//! `direction` is `Ltr` when the inline axis runs top-to-bottom (all vertical modes except
//! `sideways-lr`, modulo `direction: rtl`). Line-relative `left`/`right` keywords
//! (`float`, `clear`, `text-align`) refer to the line-left/line-right sides, which are the
//! physical top/bottom for every vertical mode except `sideways-lr`, where they are swapped.

use style::logical_geometry::{PhysicalSide, WritingMode};

/// Axis mapping for a Takia algorithm running in the axes of a [`WritingMode`].
pub trait WritingModeExt: Copy {
    /// Whether Takia's axes are swapped relative to the physical axes (a vertical writing mode).
    fn swaps_axes(self) -> bool;

    /// Whether the line-left side is physically the bottom (only `sideways-lr`).
    fn line_left_is_bottom(self) -> bool;

    /// Takia `direction` of a box with writing mode `own` laid out in `self`: `Ltr` if the box's
    /// start edge along `self`'s inline axis is Takia's left side. For a box orthogonal to `self`
    /// that edge is its block-start side.
    fn direction_of(self, own: WritingMode) -> takia::Direction;

    /// Takia `direction` of a box laid out in its own writing mode.
    fn direction(self) -> takia::Direction;

    /// Whether the `left` alignment keyword (line-left) is Takia's `end` in the inline axis.
    fn inline_left_is_end(self) -> bool;

    /// Whether the physical `left` is Takia's `end` in the block axis (`vertical-rl`/`sideways-rl`).
    fn block_left_is_end(self) -> bool;

    /// Whether the start edge of a box with writing mode `own` along `self`'s block axis is
    /// `self`'s block-end side: its block-start for parallel writing modes, its inline-start for
    /// orthogonal ones.
    fn block_start_is_reversed(self, own: WritingMode) -> bool;

    /// Map a physical rect (`left`/`right`/`top`/`bottom` are the physical sides) into Takia's axes.
    fn logical_rect<T>(self, physical: takia::Rect<T>) -> takia::Rect<T>;

    /// Map a rect in Takia's axes back to physical sides (the inverse of [`logical_rect`]).
    ///
    /// [`logical_rect`]: WritingModeExt::logical_rect
    fn physical_rect<T>(self, logical: takia::Rect<T>) -> takia::Rect<T>;

    /// Map a physical size into Takia's axes.
    fn logical_size<T>(self, physical: takia::Size<T>) -> takia::Size<T>;

    /// Map a physical point into Takia's axes.
    fn logical_point<T>(self, physical: takia::Point<T>) -> takia::Point<T>;

    /// Map a physical aspect ratio (width / height) into Takia's axes.
    fn logical_aspect_ratio(self, physical: Option<f32>) -> Option<f32>;

    /// Map a line-relative `float` into Takia's axes.
    #[cfg(feature = "floats")]
    fn logical_float(self, float: takia::Float) -> takia::Float;

    /// Map a line-relative `clear` into Takia's axes.
    #[cfg(feature = "floats")]
    fn logical_clear(self, clear: takia::Clear) -> takia::Clear;

    /// Map a line-relative legacy `text-align` into Takia's axes.
    #[cfg(feature = "block")]
    fn logical_text_align(self, align: takia::TextAlign) -> takia::TextAlign;

    /// Transpose a concrete physical [`takia::Style`] into Takia's axes for this writing mode.
    fn transpose_style<S: takia::CheapCloneStr>(self, style: &mut takia::Style<S>);
}

/// The physical side that is Takia's left (inline-axis start, `x = 0`) in `wm`.
#[inline]
fn takia_left_side(wm: WritingMode) -> PhysicalSide {
    if wm.swaps_axes() {
        PhysicalSide::Top
    } else {
        PhysicalSide::Left
    }
}

impl WritingModeExt for WritingMode {
    #[inline(always)]
    fn swaps_axes(self) -> bool {
        cfg!(feature = "writing-mode") && self.is_vertical()
    }

    #[inline]
    fn line_left_is_bottom(self) -> bool {
        cfg!(feature = "writing-mode") && self.contains(WritingMode::WRITING_MODE_SIDEWAYS_LR)
    }

    #[inline]
    fn direction_of(self, own: WritingMode) -> takia::Direction {
        let ltr = if cfg!(feature = "writing-mode") {
            let own_start = if own.is_vertical() == self.is_vertical() {
                own.inline_start_physical_side()
            } else {
                own.block_start_physical_side()
            };
            own_start == takia_left_side(self)
        } else {
            own.is_bidi_ltr()
        };
        if ltr {
            takia::Direction::Ltr
        } else {
            takia::Direction::Rtl
        }
    }

    #[inline]
    fn direction(self) -> takia::Direction {
        self.direction_of(self)
    }

    #[inline]
    fn inline_left_is_end(self) -> bool {
        (self.direction() == takia::Direction::Rtl) != self.line_left_is_bottom()
    }

    #[inline]
    fn block_left_is_end(self) -> bool {
        self.swaps_axes() && self.block_start_physical_side() == PhysicalSide::Right
    }

    #[inline]
    fn block_start_is_reversed(self, own: WritingMode) -> bool {
        if !cfg!(feature = "writing-mode") {
            return false;
        }
        let own_start = if own.is_vertical() == self.is_vertical() {
            own.block_start_physical_side()
        } else {
            own.inline_start_physical_side()
        };
        own_start != self.block_start_physical_side()
    }

    #[inline]
    fn logical_rect<T>(self, physical: takia::Rect<T>) -> takia::Rect<T> {
        if !self.swaps_axes() {
            return physical;
        }
        let (block_start, block_end) = if self.is_vertical_lr() {
            (physical.left, physical.right)
        } else {
            (physical.right, physical.left)
        };
        takia::Rect {
            left: physical.top,
            right: physical.bottom,
            top: block_start,
            bottom: block_end,
        }
    }

    #[inline]
    fn physical_rect<T>(self, logical: takia::Rect<T>) -> takia::Rect<T> {
        if !self.swaps_axes() {
            return logical;
        }
        let (left, right) = if self.is_vertical_lr() {
            (logical.top, logical.bottom)
        } else {
            (logical.bottom, logical.top)
        };
        takia::Rect {
            left,
            right,
            top: logical.left,
            bottom: logical.right,
        }
    }

    #[inline(always)]
    fn logical_size<T>(self, physical: takia::Size<T>) -> takia::Size<T> {
        if self.swaps_axes() {
            physical.transpose()
        } else {
            physical
        }
    }

    #[inline(always)]
    fn logical_point<T>(self, physical: takia::Point<T>) -> takia::Point<T> {
        if self.swaps_axes() {
            physical.transpose()
        } else {
            physical
        }
    }

    #[inline(always)]
    fn logical_aspect_ratio(self, physical: Option<f32>) -> Option<f32> {
        if self.swaps_axes() {
            physical.map(|ratio| 1.0 / ratio)
        } else {
            physical
        }
    }

    #[cfg(feature = "floats")]
    #[inline]
    fn logical_float(self, float: takia::Float) -> takia::Float {
        if self.line_left_is_bottom() {
            match float {
                takia::Float::Left => takia::Float::Right,
                takia::Float::Right => takia::Float::Left,
                takia::Float::None => takia::Float::None,
            }
        } else {
            float
        }
    }

    #[cfg(feature = "floats")]
    #[inline]
    fn logical_clear(self, clear: takia::Clear) -> takia::Clear {
        if self.line_left_is_bottom() {
            match clear {
                takia::Clear::Left => takia::Clear::Right,
                takia::Clear::Right => takia::Clear::Left,
                other => other,
            }
        } else {
            clear
        }
    }

    #[cfg(feature = "block")]
    #[inline]
    fn logical_text_align(self, align: takia::TextAlign) -> takia::TextAlign {
        if self.line_left_is_bottom() {
            match align {
                takia::TextAlign::LegacyLeft => takia::TextAlign::LegacyRight,
                takia::TextAlign::LegacyRight => takia::TextAlign::LegacyLeft,
                other => other,
            }
        } else {
            align
        }
    }

    fn transpose_style<S: takia::CheapCloneStr>(self, style: &mut takia::Style<S>) {
        if !self.swaps_axes() {
            return;
        }
        style.direction = self.direction();
        style.overflow = style.overflow.transpose();
        #[cfg(feature = "floats")]
        {
            style.float = self.logical_float(style.float);
            style.clear = self.logical_clear(style.clear);
        }
        #[cfg(feature = "block")]
        {
            style.text_align = self.logical_text_align(style.text_align);
        }
        style.size = style.size.transpose();
        style.min_size = style.min_size.transpose();
        style.max_size = style.max_size.transpose();
        style.aspect_ratio = self.logical_aspect_ratio(style.aspect_ratio);
        style.inset = self.logical_rect(style.inset);
        style.margin = self.logical_rect(style.margin);
        style.padding = self.logical_rect(style.padding);
        style.border = self.logical_rect(style.border);
    }
}
