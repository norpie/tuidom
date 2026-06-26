use crossterm::event::{
    KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
    MediaKeyCode as CrosstermMediaKeyCode, ModifierKeyCode as CrosstermModifierKeyCode,
};

use super::KeyEvent;

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
    KeyEvent::new(convert_key_code(key.code))
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
        KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent, KeyModifiers,
        MediaKeyCode as CrosstermMediaKeyCode, ModifierKeyCode as CrosstermModifierKeyCode,
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
    fn converts_key_event_code() {
        let event = CrosstermKeyEvent::new(CrosstermKeyCode::Delete, KeyModifiers::CONTROL);

        assert_eq!(convert_key_event(event).code, KeyCode::Delete);
    }
}
