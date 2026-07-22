use std::ops::{BitOr, BitOrAssign};

use crossterm::event::{
    KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
    KeyModifiers as CrosstermKeyModifiers, MediaKeyCode as CrosstermMediaKeyCode,
    ModifierKeyCode as CrosstermModifierKeyCode,
};

use super::KeyEvent;

/// Modifier keys held during a key or mouse event.
///
/// Only Shift, Control, and Alt are represented. Terminals report Super, Hyper, and Meta
/// solely under the kitty keyboard protocol, so a binding on one of them would match on
/// almost no terminal while looking correct in the source — the same reason key release
/// is not reported. Those bits are dropped at conversion rather than exposed as flags
/// that never set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct KeyModifiers(u8);

impl KeyModifiers {
    /// The shift key.
    pub const SHIFT: Self = Self(0b001);
    /// The control key.
    pub const CONTROL: Self = Self(0b010);
    /// The alt (option) key.
    pub const ALT: Self = Self(0b100);

    /// No modifiers held.
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Whether no modifiers are held.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Whether every modifier in `other` is held.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether any modifier in `other` is held.
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// This set with every modifier in `other` removed.
    pub const fn without(self, other: Self) -> Self {
        Self(self.0 & !other.0)
    }

    /// Take the modifiers this engine represents, dropping the kitty-only ones.
    pub(crate) fn from_crossterm(modifiers: CrosstermKeyModifiers) -> Self {
        let mut converted = Self::empty();
        if modifiers.contains(CrosstermKeyModifiers::SHIFT) {
            converted |= Self::SHIFT;
        }
        if modifiers.contains(CrosstermKeyModifiers::CONTROL) {
            converted |= Self::CONTROL;
        }
        if modifiers.contains(CrosstermKeyModifiers::ALT) {
            converted |= Self::ALT;
        }
        converted
    }
}

impl BitOr for KeyModifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for KeyModifiers {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Represents a media key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKeyCode {
    /// Play media key.
    Play,
    /// Pause media key.
    Pause,
    /// Play/Pause media key.
    PlayPause,
    /// Reverse media key.
    Reverse,
    /// Stop media key.
    Stop,
    /// Fast-forward media key.
    FastForward,
    /// Rewind media key.
    Rewind,
    /// Next-track media key.
    TrackNext,
    /// Previous-track media key.
    TrackPrevious,
    /// Record media key.
    Record,
    /// Lower-volume media key.
    LowerVolume,
    /// Raise-volume media key.
    RaiseVolume,
    /// Mute media key.
    MuteVolume,
}

/// Represents a modifier key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKeyCode {
    /// Left Shift key.
    LeftShift,
    /// Left Control key.
    LeftControl,
    /// Left Alt key.
    LeftAlt,
    /// Left Super key.
    LeftSuper,
    /// Left Hyper key.
    LeftHyper,
    /// Left Meta key.
    LeftMeta,
    /// Right Shift key.
    RightShift,
    /// Right Control key.
    RightControl,
    /// Right Alt key.
    RightAlt,
    /// Right Super key.
    RightSuper,
    /// Right Hyper key.
    RightHyper,
    /// Right Meta key.
    RightMeta,
    /// ISO Level 3 Shift key.
    IsoLevel3Shift,
    /// ISO Level 5 Shift key.
    IsoLevel5Shift,
}

/// Represents a keyboard key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// Backspace key.
    Backspace,
    /// Enter key.
    Enter,
    /// Left arrow key.
    Left,
    /// Right arrow key.
    Right,
    /// Up arrow key.
    Up,
    /// Down arrow key.
    Down,
    /// Home key.
    Home,
    /// End key.
    End,
    /// Page up key.
    PageUp,
    /// Page down key.
    PageDown,
    /// Tab key.
    Tab,
    /// Shift + Tab key.
    BackTab,
    /// Delete key.
    Delete,
    /// Insert key.
    Insert,
    /// Function key.
    F(u8),
    /// Character key.
    Char(char),
    /// Null key.
    Null,
    /// Escape key.
    Esc,
    /// Caps Lock key.
    CapsLock,
    /// Scroll Lock key.
    ScrollLock,
    /// Num Lock key.
    NumLock,
    /// Print Screen key.
    PrintScreen,
    /// Pause key.
    Pause,
    /// Menu key.
    Menu,
    /// Keypad Begin key.
    KeypadBegin,
    /// Media key.
    Media(MediaKeyCode),
    /// Modifier key.
    Modifier(ModifierKeyCode),
}

pub(crate) fn convert_key_event(key: CrosstermKeyEvent) -> KeyEvent {
    KeyEvent::with_modifiers(
        convert_key_code(key.code),
        KeyModifiers::from_crossterm(key.modifiers),
    )
}

fn convert_key_code(code: CrosstermKeyCode) -> KeyCode {
    match code {
        CrosstermKeyCode::Backspace => KeyCode::Backspace,
        CrosstermKeyCode::Enter => KeyCode::Enter,
        CrosstermKeyCode::Left => KeyCode::Left,
        CrosstermKeyCode::Right => KeyCode::Right,
        CrosstermKeyCode::Up => KeyCode::Up,
        CrosstermKeyCode::Down => KeyCode::Down,
        CrosstermKeyCode::Home => KeyCode::Home,
        CrosstermKeyCode::End => KeyCode::End,
        CrosstermKeyCode::PageUp => KeyCode::PageUp,
        CrosstermKeyCode::PageDown => KeyCode::PageDown,
        CrosstermKeyCode::Tab => KeyCode::Tab,
        CrosstermKeyCode::BackTab => KeyCode::BackTab,
        CrosstermKeyCode::Delete => KeyCode::Delete,
        CrosstermKeyCode::Insert => KeyCode::Insert,
        CrosstermKeyCode::F(n) => KeyCode::F(n),
        CrosstermKeyCode::Char(c) => KeyCode::Char(c),
        CrosstermKeyCode::Null => KeyCode::Null,
        CrosstermKeyCode::Esc => KeyCode::Esc,
        CrosstermKeyCode::CapsLock => KeyCode::CapsLock,
        CrosstermKeyCode::ScrollLock => KeyCode::ScrollLock,
        CrosstermKeyCode::NumLock => KeyCode::NumLock,
        CrosstermKeyCode::PrintScreen => KeyCode::PrintScreen,
        CrosstermKeyCode::Pause => KeyCode::Pause,
        CrosstermKeyCode::Menu => KeyCode::Menu,
        CrosstermKeyCode::KeypadBegin => KeyCode::KeypadBegin,
        CrosstermKeyCode::Media(media) => KeyCode::Media(convert_media_key_code(media)),
        CrosstermKeyCode::Modifier(modifier) => {
            KeyCode::Modifier(convert_modifier_key_code(modifier))
        }
    }
}

fn convert_media_key_code(code: CrosstermMediaKeyCode) -> MediaKeyCode {
    match code {
        CrosstermMediaKeyCode::Play => MediaKeyCode::Play,
        CrosstermMediaKeyCode::Pause => MediaKeyCode::Pause,
        CrosstermMediaKeyCode::PlayPause => MediaKeyCode::PlayPause,
        CrosstermMediaKeyCode::Reverse => MediaKeyCode::Reverse,
        CrosstermMediaKeyCode::Stop => MediaKeyCode::Stop,
        CrosstermMediaKeyCode::FastForward => MediaKeyCode::FastForward,
        CrosstermMediaKeyCode::Rewind => MediaKeyCode::Rewind,
        CrosstermMediaKeyCode::TrackNext => MediaKeyCode::TrackNext,
        CrosstermMediaKeyCode::TrackPrevious => MediaKeyCode::TrackPrevious,
        CrosstermMediaKeyCode::Record => MediaKeyCode::Record,
        CrosstermMediaKeyCode::LowerVolume => MediaKeyCode::LowerVolume,
        CrosstermMediaKeyCode::RaiseVolume => MediaKeyCode::RaiseVolume,
        CrosstermMediaKeyCode::MuteVolume => MediaKeyCode::MuteVolume,
    }
}

fn convert_modifier_key_code(code: CrosstermModifierKeyCode) -> ModifierKeyCode {
    match code {
        CrosstermModifierKeyCode::LeftShift => ModifierKeyCode::LeftShift,
        CrosstermModifierKeyCode::LeftControl => ModifierKeyCode::LeftControl,
        CrosstermModifierKeyCode::LeftAlt => ModifierKeyCode::LeftAlt,
        CrosstermModifierKeyCode::LeftSuper => ModifierKeyCode::LeftSuper,
        CrosstermModifierKeyCode::LeftHyper => ModifierKeyCode::LeftHyper,
        CrosstermModifierKeyCode::LeftMeta => ModifierKeyCode::LeftMeta,
        CrosstermModifierKeyCode::RightShift => ModifierKeyCode::RightShift,
        CrosstermModifierKeyCode::RightControl => ModifierKeyCode::RightControl,
        CrosstermModifierKeyCode::RightAlt => ModifierKeyCode::RightAlt,
        CrosstermModifierKeyCode::RightSuper => ModifierKeyCode::RightSuper,
        CrosstermModifierKeyCode::RightHyper => ModifierKeyCode::RightHyper,
        CrosstermModifierKeyCode::RightMeta => ModifierKeyCode::RightMeta,
        CrosstermModifierKeyCode::IsoLevel3Shift => ModifierKeyCode::IsoLevel3Shift,
        CrosstermModifierKeyCode::IsoLevel5Shift => ModifierKeyCode::IsoLevel5Shift,
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{
        KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
        KeyModifiers as CrosstermKeyModifiers, MediaKeyCode as CrosstermMediaKeyCode,
        ModifierKeyCode as CrosstermModifierKeyCode,
    };

    use super::*;

    #[test]
    fn converts_common_key_codes_without_fallback() {
        assert_eq!(
            convert_key_code(CrosstermKeyCode::Char('?')),
            KeyCode::Char('?')
        );
        assert_eq!(convert_key_code(CrosstermKeyCode::Enter), KeyCode::Enter);
        assert_eq!(
            convert_key_code(CrosstermKeyCode::Backspace),
            KeyCode::Backspace
        );
        assert_eq!(convert_key_code(CrosstermKeyCode::Left), KeyCode::Left);
        assert_eq!(convert_key_code(CrosstermKeyCode::Right), KeyCode::Right);
        assert_eq!(convert_key_code(CrosstermKeyCode::Up), KeyCode::Up);
        assert_eq!(convert_key_code(CrosstermKeyCode::Down), KeyCode::Down);
        assert_eq!(convert_key_code(CrosstermKeyCode::F(12)), KeyCode::F(12));
    }

    #[test]
    fn converts_extended_key_codes() {
        assert_eq!(
            convert_key_code(CrosstermKeyCode::Media(CrosstermMediaKeyCode::PlayPause)),
            KeyCode::Media(MediaKeyCode::PlayPause)
        );
        assert_eq!(
            convert_key_code(CrosstermKeyCode::Modifier(
                CrosstermModifierKeyCode::RightAlt
            )),
            KeyCode::Modifier(ModifierKeyCode::RightAlt)
        );
        assert_eq!(
            convert_key_code(CrosstermKeyCode::KeypadBegin),
            KeyCode::KeypadBegin
        );
    }

    #[test]
    fn converts_key_event_code_and_modifiers() {
        let event =
            CrosstermKeyEvent::new(CrosstermKeyCode::Delete, CrosstermKeyModifiers::CONTROL);

        let converted = convert_key_event(event);

        assert_eq!(converted.code, KeyCode::Delete);
        assert_eq!(converted.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn converts_each_represented_modifier() {
        for (crossterm, expected) in [
            (CrosstermKeyModifiers::SHIFT, KeyModifiers::SHIFT),
            (CrosstermKeyModifiers::CONTROL, KeyModifiers::CONTROL),
            (CrosstermKeyModifiers::ALT, KeyModifiers::ALT),
        ] {
            let event = CrosstermKeyEvent::new(CrosstermKeyCode::Char('a'), crossterm);

            assert_eq!(convert_key_event(event).modifiers, expected);
        }
    }

    #[test]
    fn converts_combined_modifiers() {
        let event = CrosstermKeyEvent::new(
            CrosstermKeyCode::Char('a'),
            CrosstermKeyModifiers::CONTROL | CrosstermKeyModifiers::SHIFT,
        );

        let modifiers = convert_key_event(event).modifiers;

        assert!(modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
        assert!(!modifiers.contains(KeyModifiers::ALT));
    }

    /// Super, hyper, and meta only reach us under the kitty keyboard protocol. Dropping
    /// them must not alias onto a modifier we do represent.
    #[test]
    fn drops_kitty_only_modifiers_without_aliasing() {
        for kitty in [
            CrosstermKeyModifiers::SUPER,
            CrosstermKeyModifiers::HYPER,
            CrosstermKeyModifiers::META,
        ] {
            let event = CrosstermKeyEvent::new(CrosstermKeyCode::Char('a'), kitty);

            assert!(convert_key_event(event).modifiers.is_empty());
        }
    }

    /// A control chord arrives as its plain letter, so the code alone cannot tell ctrl+a
    /// from a typed `a` — the modifier is the only thing that distinguishes them.
    #[test]
    fn control_chord_keeps_its_plain_letter_code() {
        let event =
            CrosstermKeyEvent::new(CrosstermKeyCode::Char('a'), CrosstermKeyModifiers::CONTROL);

        let converted = convert_key_event(event);

        assert_eq!(converted.code, KeyCode::Char('a'));
        assert_eq!(converted.modifiers, KeyModifiers::CONTROL);
    }

    #[test]
    fn modifier_set_operations() {
        let both = KeyModifiers::CONTROL | KeyModifiers::SHIFT;

        assert!(both.contains(KeyModifiers::CONTROL));
        assert!(both.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT));
        assert!(!both.contains(KeyModifiers::CONTROL | KeyModifiers::ALT));
        assert_eq!(both.without(KeyModifiers::SHIFT), KeyModifiers::CONTROL);
        assert!(KeyModifiers::empty().is_empty());
    }
}
