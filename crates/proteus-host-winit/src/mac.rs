//! What the host reads from macOS itself, where winit doesn't report it.

/// The macOS event source whose state combines every source's: the keyboard
/// as the user sees it. `kCGEventSourceStateCombinedSessionState`.
const COMBINED_SESSION_STATE: i32 = 0;
/// Caps Lock's flag in an event source's modifier flags:
/// `kCGEventFlagMaskAlphaShift`.
const ALPHA_SHIFT: u64 = 0x0001_0000;

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    /// The modifier flags of the event source `state_id`.
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

/// Whether Caps Lock is on.
pub(crate) fn caps_lock_on() -> bool {
    // SAFETY: `CGEventSourceFlagsState` takes a source state ID by value and
    // returns flags by value; it touches no memory of ours, and any thread
    // may call it.
    let flags = unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) };
    flags & ALPHA_SHIFT != 0
}

#[cfg(test)]
mod tests {
    // The call links against CoreGraphics and runs, whichever way Caps Lock
    // happens to be.
    #[test]
    fn caps_lock_can_be_read() {
        let _ = super::caps_lock_on();
    }
}
