//! Caps Lock, reported the same way on every host.

use proteus_sdk::{Code, Key, KeyEvent, KeyState, Modifiers, NamedKey};

/// Turns what a platform says about Caps Lock into a press and a release at
/// each tap, which is how a host reports it to the app on every platform.
///
/// Platforms differ. Windows and Linux report a press and a release at each
/// tap. A browser on a Mac reports a press as Caps Lock turns on and a
/// release as it turns off, so it seems held all the while it's on. winit on
/// a Mac reports nothing at all. A host that can tell whether Caps Lock is
/// on, from the browser's `getModifierState` or the operating system, passes
/// that here instead of the platform's own Caps Lock events, and sends the
/// app a tap whenever it changes.
#[derive(Debug, Default, Clone)]
pub struct CapsLockTaps {
    /// Whether Caps Lock was on, when last noted.
    on: Option<bool>,
}

impl CapsLockTaps {
    /// Notes whether Caps Lock is on, without a tap: for a host that reads
    /// it apart from Caps Lock's own events, such as on any other key, or
    /// as its window gains focus, when it may have changed in another app.
    pub fn note(&mut self, on: bool) {
        self.on = Some(on);
    }

    /// Notes whether Caps Lock is on, as a host that polls it finds, and
    /// returns a tap if it changed since last noted. The first poll only
    /// notes it.
    pub fn poll(&mut self, on: bool, modifiers: Modifiers) -> Option<[KeyEvent; 2]> {
        let changed = self.on.is_some_and(|was| was != on);
        self.on = Some(on);
        changed.then(|| tap(modifiers))
    }

    /// Notes whether Caps Lock is on, as one of its own key events says, and
    /// returns a tap if it changed since last noted, or if it hasn't been
    /// noted yet: a Caps Lock event that changes nothing, such as the release
    /// after a press on Windows, is part of a tap already sent.
    pub fn key(&mut self, on: bool, modifiers: Modifiers) -> Option<[KeyEvent; 2]> {
        let changed = self.on != Some(on);
        self.on = Some(on);
        changed.then(|| tap(modifiers))
    }
}

/// A press and a release of Caps Lock.
fn tap(modifiers: Modifiers) -> [KeyEvent; 2] {
    let event = |state| {
        KeyEvent::new(
            Key::Named(NamedKey::CapsLock),
            Code::CapsLock,
            state,
            modifiers,
            None,
        )
    };
    [event(KeyState::Pressed), event(KeyState::Released)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_tap(events: Option<[KeyEvent; 2]>) -> bool {
        events.is_some_and(|[press, release]| {
            press.state == KeyState::Pressed
                && release.state == KeyState::Released
                && press.code == Code::CapsLock
        })
    }

    // Polling, as natively on a Mac: the first poll only notes the state,
    // and each change after is one tap.
    #[test]
    fn a_poll_taps_once_per_change() {
        let mut caps = CapsLockTaps::default();
        let m = Modifiers::default();
        assert!(caps.poll(true, m).is_none(), "the first poll only notes it");
        assert!(caps.poll(true, m).is_none());
        assert!(is_tap(caps.poll(false, m)));
        assert!(caps.poll(false, m).is_none());
        assert!(is_tap(caps.poll(true, m)));
    }

    // A browser on Windows: a keydown and a keyup at each tap, both saying
    // the new state, so the keyup changes nothing.
    #[test]
    fn a_press_and_release_at_each_tap_is_one_tap() {
        let mut caps = CapsLockTaps::default();
        let m = Modifiers::default();
        assert!(is_tap(caps.key(true, m)), "keydown, turning it on");
        assert!(caps.key(true, m).is_none(), "its keyup");
        assert!(is_tap(caps.key(false, m)), "keydown, turning it off");
        assert!(caps.key(false, m).is_none(), "its keyup");
    }

    // A browser on a Mac: a keydown as it turns on, a keyup as it turns off.
    #[test]
    fn a_press_to_turn_on_and_a_release_to_turn_off_is_a_tap_each() {
        let mut caps = CapsLockTaps::default();
        let m = Modifiers::default();
        caps.note(false);
        assert!(is_tap(caps.key(true, m)), "keydown, turning it on");
        assert!(is_tap(caps.key(false, m)), "keyup, turning it off");
    }

    // A state noted from another key, or as the window gains focus, makes
    // no tap, so a change made in another app isn't one.
    #[test]
    fn a_noted_state_makes_no_tap() {
        let mut caps = CapsLockTaps::default();
        let m = Modifiers::default();
        caps.poll(false, m);
        caps.note(true);
        assert!(caps.poll(true, m).is_none());
    }
}
