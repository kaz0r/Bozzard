//! Assignable gameplay keys. A Blueprint binds `On Input Pressed` / `Input Held` to a key by
//! name, so a scene chooses its own buttons instead of the engine's fixed seven. Bit `i` of
//! `GameplayInput::keys` is `BOUND_KEYS[i]`; the seven axis/edge aliases keep the top of the
//! mask, out of the way of assignable keys.

/// Names a scene may bind, indexed by bit position in `GameplayInput::keys`.
pub const BOUND_KEYS: &[&str] = &[
    "A",
    "B",
    "C",
    "D",
    "E",
    "F",
    "G",
    "H",
    "I",
    "J",
    "K",
    "L",
    "M",
    "N",
    "O",
    "P",
    "Q",
    "R",
    "S",
    "T",
    "U",
    "V",
    "W",
    "X",
    "Y",
    "Z",
    "0",
    "1",
    "2",
    "3",
    "4",
    "5",
    "6",
    "7",
    "8",
    "9",
    "Space",
    "Enter",
    "Escape",
    "Tab",
    "Backspace",
    "Delete",
    "Insert",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Shift",
    "Ctrl",
    "Alt",
    "ArrowUp",
    "ArrowDown",
    "ArrowLeft",
    "ArrowRight",
    "F1",
    "F2",
    "F3",
    "F4",
    "F5",
    "F6",
    "F7",
    "F8",
    "F9",
    "F10",
    "F11",
    "F12",
    "MouseLeft",
    "MouseRight",
    "MouseMiddle",
];
/// The original fixed bindings, kept so older scenes read the same axes and edges.
pub const KEY_ALIASES: [&str; 7] = [
    "forward", "backward", "left", "right", "jump", "fire", "interact",
];
/// Aliases take the top of the mask so assignable keys always start at bit zero.
const ALIAS_BASE: u32 = u128::BITS - KEY_ALIASES.len() as u32 - 1;
/// App spellings the prefix and suffix rules cannot reach: winit splits the modifiers.
const SPELLINGS: [(&str, &str); 6] = [
    ("shiftleft", "Shift"),
    ("shiftright", "Shift"),
    ("controlleft", "Ctrl"),
    ("controlright", "Ctrl"),
    ("altleft", "Alt"),
    ("altright", "Alt"),
];

/// The canonical name for an authored or app-reported key, or `None` when nothing is bound to
/// it. Spellings differ between author, winit (`KeyF`, `Digit1`, `ArrowLeft`) and egui (`F`,
/// `Num1`, `ArrowLeft`), so decoration is stripped before matching.
pub fn canonical(requested: &str) -> Option<&'static str> {
    let lowered = requested.trim().to_ascii_lowercase();
    if let Some(alias) = KEY_ALIASES.iter().find(|alias| **alias == lowered) {
        return Some(alias);
    }
    let exact = |name: &str| {
        BOUND_KEYS
            .iter()
            .copied()
            .find(|key| key.eq_ignore_ascii_case(name))
    };
    if let Some(name) = exact(&lowered) {
        return Some(name);
    }
    if let Some((_, name)) = SPELLINGS.iter().find(|(spelling, _)| *spelling == lowered) {
        return Some(name);
    }
    // `KeyF`, `Digit1`, `Num1`, `ArrowLeft`: strip the decoration, then match.
    for prefix in ["key", "digit", "num"] {
        if let Some(rest) = lowered.strip_prefix(prefix)
            && let Some(name) = exact(rest)
        {
            return Some(name);
        }
    }
    // `ShiftLeft`: modifiers arrive per side but bind as one button.
    for suffix in ["left", "right"] {
        if let Some(rest) = lowered.strip_suffix(suffix)
            && let Some(name) = exact(rest)
        {
            return Some(name);
        }
    }
    None
}

/// Bit for an assignable key name, or `0` when the name binds nothing.
pub fn bit(requested: &str) -> u128 {
    canonical(requested)
        .and_then(|name| BOUND_KEYS.iter().position(|key| *key == name))
        .map_or(0, |index| 1 << index)
}

/// Bit for alias `index` of `KEY_ALIASES`.
pub fn alias_bit(index: usize) -> u128 {
    1 << (ALIAS_BASE + index as u32)
}

/// Position of an alias name in `KEY_ALIASES`.
pub fn alias_index(requested: &str) -> Option<usize> {
    let lowered = requested.trim().to_ascii_lowercase();
    KEY_ALIASES.iter().position(|alias| *alias == lowered)
}

/// Whether alias `index` is active this tick. Every alias reads the axis, edge or button the
/// engine has always exposed, so scenes that used the fixed seven keep their behavior.
pub fn alias_active(index: usize, input: crate::GameplayInput) -> bool {
    match index {
        0 => input.movement[1] > 0.,
        1 => input.movement[1] < 0.,
        2 => input.movement[0] < 0.,
        3 => input.movement[0] > 0.,
        4 => input.jump,
        5 => input.fire,
        _ => input.interact,
    }
}

/// Name of assignable key `index`, for the editor picker and the unresolved-key hint.
pub fn name(index: usize) -> &'static str {
    BOUND_KEYS[index]
}

/// Names of the assignable keys set in `mask`, in binding order.
pub fn mask_names(mask: u128) -> impl Iterator<Item = &'static str> {
    BOUND_KEYS
        .iter()
        .copied()
        .enumerate()
        .filter_map(move |(index, name)| (mask & (1 << index) != 0).then_some(name))
}

/// Every name a scene may bind: the aliases first, then the assignable keys.
pub fn authorable() -> impl Iterator<Item = &'static str> {
    KEY_ALIASES.into_iter().chain(BOUND_KEYS.iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_mask_holds_every_name_in_one_u128() {
        assert!(BOUND_KEYS.len() + KEY_ALIASES.len() < u128::BITS as usize);
        for (index, bound) in BOUND_KEYS.iter().enumerate() {
            // Also asserts no alias is shadowed by an assignable name, and vice versa.
            assert_eq!(canonical(bound), Some(*bound), "{bound}");
            assert_eq!(bit(bound), 1 << index, "{bound}");
            assert_eq!(name(index), *bound);
        }
        // The movement aliases keep their own names instead of stealing a key.
        assert_eq!(canonical("left"), Some("left"));
        assert_eq!(canonical("Left"), Some("left"));
        assert_eq!(canonical("ArrowLeft"), Some("ArrowLeft"));
        assert_eq!(bit("interact"), 0);
        assert_eq!(canonical("E"), Some("E"));
        assert_eq!(alias_bit(0) & bit("A"), 0);
    }
    #[test]
    fn canonical_accepts_author_and_app_spellings() {
        // Author, winit and egui write the same button three ways.
        for (requested, expected) in [
            ("F", "F"),
            ("f", "F"),
            ("KeyF", "F"),
            ("7", "7"),
            ("Digit7", "7"),
            ("Num7", "7"),
            ("Space", "Space"),
            ("Escape", "Escape"),
            ("ArrowLeft", "ArrowLeft"),
            ("F12", "F12"),
            ("MouseLeft", "MouseLeft"),
            ("mouseleft", "MouseLeft"),
            ("ShiftLeft", "Shift"),
            ("ControlRight", "Ctrl"),
            ("AltLeft", "Alt"),
            (" Shift ", "Shift"),
            ("forward", "forward"),
            ("Interact", "interact"),
        ] {
            assert_eq!(canonical(requested), Some(expected), "{requested}");
        }
        for unknown in ["", "  ", "Foo", "KeyFoo", "Numpad1", "SuperLeft", "MouseX1"] {
            assert_eq!(canonical(unknown), None, "{unknown}");
        }
    }
}
