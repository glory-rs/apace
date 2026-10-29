use apace_traits::events::{
    ApaceImeEvent, ApaceKeyEvent, ApacePointerId, KeyState, PointerDetails,
};
use apace_traits::shell::ColorScheme;
use keyboard_types::{Code, Key, Modifiers, NamedKey};
use wingy::event::KeyEvent as WingyKeyEvent;
use wingy::event::{ButtonSource, ElementState};
use wingy::event::{Ime, PointerKind, PointerSource};
use wingy::keyboard::Key as WingyKey;
use wingy::keyboard::ModifiersState as WingyModifiers;
use wingy::keyboard::PhysicalKey as WingyPhysicalKey;
use wingy::window::Theme;

pub(crate) fn theme_to_color_scheme(theme: Theme) -> ColorScheme {
    match theme {
        Theme::Light => ColorScheme::Light,
        Theme::Dark => ColorScheme::Dark,
    }
}

pub(crate) fn color_scheme_to_theme(scheme: ColorScheme) -> Theme {
    match scheme {
        ColorScheme::Light => Theme::Light,
        ColorScheme::Dark => Theme::Dark,
    }
}

pub(crate) fn wingy_ime_to_apace(event: Ime) -> ApaceImeEvent {
    match event {
        Ime::Enabled => ApaceImeEvent::Enabled,
        Ime::Disabled => ApaceImeEvent::Disabled,
        Ime::Preedit(text, cursor) => ApaceImeEvent::Preedit(text, cursor),
        Ime::Commit(text) => ApaceImeEvent::Commit(text),
        Ime::DeleteSurrounding {
            before_bytes,
            after_bytes,
        } => ApaceImeEvent::DeleteSurrounding {
            before_bytes,
            after_bytes,
        },
    }
}

pub(crate) fn wingy_key_event_to_apace(
    event: &WingyKeyEvent,
    mods: WingyModifiers,
) -> ApaceKeyEvent {
    ApaceKeyEvent {
        key: wingy_key_to_kbt_key(&event.logical_key),
        code: wingy_physical_key_to_kbt_code(&event.physical_key),
        modifiers: wingy_modifiers_to_kbt_modifiers(mods),
        location: event.location,
        is_auto_repeating: event.repeat,
        is_composing: false,
        state: match event.state {
            ElementState::Pressed => KeyState::Pressed,
            ElementState::Released => KeyState::Released,
        },
        text: event.text.clone(),
    }
}

pub(crate) fn pointer_source_to_apace(source: &PointerSource) -> ApacePointerId {
    match source {
        PointerSource::Mouse => ApacePointerId::Mouse,
        PointerSource::Touch { finger_id, .. } => {
            ApacePointerId::Finger(finger_id.into_raw() as u64)
        }

        // TODO: TabletTool and Unknown events
        PointerSource::TabletTool { .. } => ApacePointerId::Pen,
        PointerSource::Unknown => ApacePointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerSource variant: {source:?}");
            ApacePointerId::Mouse
        }
    }
}

pub(crate) fn pointer_kind_to_apace(kind: &PointerKind) -> ApacePointerId {
    match kind {
        PointerKind::Mouse => ApacePointerId::Mouse,
        PointerKind::Touch(finger_id) => ApacePointerId::Finger(finger_id.into_raw() as u64),

        // TODO: TabletTool and Unknown events
        PointerKind::TabletTool(_) => ApacePointerId::Pen,
        PointerKind::Unknown => ApacePointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerKind variant: {kind:?}");
            ApacePointerId::Mouse
        }
    }
}

pub(crate) fn button_source_to_apace(source: &ButtonSource) -> ApacePointerId {
    match source {
        ButtonSource::Mouse(_) => ApacePointerId::Mouse,
        ButtonSource::TabletTool { .. } => ApacePointerId::Pen,
        ButtonSource::Touch { finger_id, .. } => {
            ApacePointerId::Finger(finger_id.into_raw() as u64)
        }

        // TODO: Unknown events
        ButtonSource::Unknown(_) => ApacePointerId::Mouse,
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported ButtonSource variant: {source:?}");
            ApacePointerId::Mouse
        }
    }
}

pub(crate) fn pointer_source_to_apace_details(source: &PointerSource) -> PointerDetails {
    match source {
        PointerSource::Mouse => PointerDetails::default(),
        PointerSource::Unknown => PointerDetails::default(),
        PointerSource::Touch { force, .. } => PointerDetails {
            pressure: force.map(|force| force.normalized(None)).unwrap_or(0.0),
            ..PointerDetails::default()
        },
        PointerSource::TabletTool { data, .. } => PointerDetails {
            pressure: data
                .force
                .map(|force| force.normalized(data.angle))
                .unwrap_or(0.0),
            tangential_pressure: data.tangential_force.unwrap_or(0.0),
            tilt_x: data.tilt.map(|tilt| tilt.x).unwrap_or(0),
            tilt_y: data.tilt.map(|tilt| tilt.y).unwrap_or(0),
            twist: data.twist.unwrap_or(0),
            altitude: data.angle.map(|angle| angle.altitude).unwrap_or(0.0),
            azimuth: data.angle.map(|angle| angle.azimuth).unwrap_or(0.0),
        },
        _ => {
            #[cfg(feature = "tracing")]
            tracing::error!("unsupported PointerSource variant: {source:?}");
            PointerDetails::default()
        }
    }
}

pub(crate) fn wingy_modifiers_to_kbt_modifiers(wingy_modifiers: WingyModifiers) -> Modifiers {
    let mut modifiers = Modifiers::default();
    if wingy_modifiers.control_key() {
        modifiers.insert(Modifiers::CONTROL);
    }
    if wingy_modifiers.alt_key() {
        modifiers.insert(Modifiers::ALT);
    }
    if wingy_modifiers.shift_key() {
        modifiers.insert(Modifiers::SHIFT);
    }
    if wingy_modifiers.meta_key() {
        modifiers.insert(Modifiers::META);
    }
    modifiers
}

pub(crate) fn wingy_physical_key_to_kbt_code(physical_key: &WingyPhysicalKey) -> Code {
    match physical_key {
        WingyPhysicalKey::Code(code) => *code,
        WingyPhysicalKey::Unidentified(_) => Code::Unidentified,
    }
}

pub(crate) fn wingy_key_to_kbt_key(wingy_key: &WingyKey) -> Key {
    match wingy_key {
        WingyKey::Named(named_key) => Key::Named(*named_key),
        WingyKey::Character(c) => Key::Character(c.to_string()),
        WingyKey::Unidentified(_) => Key::Named(NamedKey::Unidentified),
        WingyKey::Dead(_) => Key::Named(NamedKey::Dead),
    }
}

#[cfg(test)]
mod ime_tests {
    use super::*;

    #[test]
    fn wingy_ime_events_preserve_text_and_byte_offsets() {
        assert!(matches!(
            wingy_ime_to_apace(Ime::Enabled),
            ApaceImeEvent::Enabled
        ));
        assert!(matches!(
            wingy_ime_to_apace(Ime::Disabled),
            ApaceImeEvent::Disabled
        ));
        match wingy_ime_to_apace(Ime::Preedit("a中".into(), Some((1, 4)))) {
            ApaceImeEvent::Preedit(text, cursor) => {
                assert_eq!(text, "a中");
                assert_eq!(cursor, Some((1, 4)));
            }
            event => panic!("Unexpected preedit conversion: {event:?}"),
        }
        assert!(matches!(
            wingy_ime_to_apace(Ime::Commit("中".into())),
            ApaceImeEvent::Commit(text) if text == "中"
        ));
        assert!(matches!(
            wingy_ime_to_apace(Ime::DeleteSurrounding {
                before_bytes: 3,
                after_bytes: 1
            }),
            ApaceImeEvent::DeleteSurrounding {
                before_bytes: 3,
                after_bytes: 1
            }
        ));
    }
}
