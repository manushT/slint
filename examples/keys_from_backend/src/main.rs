// Issue #11117: Keys creation from backends
//
// This example demonstrates creating `keys` values from Rust at runtime,
// using the public API added for issue #11117.

use slint::Keys;

slint::include_modules!();

fn main() {
    let window = MainWindow::new().unwrap();

    // Listen to shortcut activations from Rust
    let window_weak = window.as_weak();
    window.on_shortcut_triggered(move |action| {
        println!("Shortcut triggered: {action}");
        match action.as_str() {
            "save" => println!("  → Would save the file"),
            "undo" => println!("  → Would undo last action"),
            "refresh" => println!("  → Would refresh"),
            "zoom-in" => println!("  → Would zoom in"),
            "user-custom" => println!("  → User-configured shortcut fired!"),
            _ => {}
        }
        if let Some(window) = window_weak.upgrade() {
            let display = window.get_user_shortcut_display();
            println!("  Current user shortcut displays as: {display}");
        }
    });

    // ✅ NEW: Create `Keys` from Rust using a list of string parts!

    // Example 1: Simple Ctrl+key binding
    let ctrl_p = Keys::from_parts(["Control", "P"]).expect("valid shortcut");
    println!("Created shortcut from Rust: {ctrl_p}");

    // Example 2: Plus key (named key from the Key namespace)
    let ctrl_plus = Keys::from_parts(["Control", "Plus"]).expect("valid shortcut");
    println!("Created shortcut from Rust: {ctrl_plus}");

    // Example 3: With Shift? (matches regardless of Shift state)
    let ctrl_z_ignore_shift = Keys::from_parts(["Control", "Shift?", "Z"]).expect("valid shortcut");
    println!("Created shortcut from Rust: {ctrl_z_ignore_shift:?}");

    // Set the user-configurable shortcut from Rust at runtime!
    // This was impossible before issue #11117.
    window.set_user_shortcut(ctrl_p);

    // Bind a shortcut to the button from Rust
    let ctrl_b = Keys::from_parts(["Control", "B"]).expect("valid shortcut");
    println!("Button shortcut: {ctrl_b}");
    window.set_button_shortcut(ctrl_b);

    window.on_button_clicked(|| {
        println!("Button was activated!");
    });

    window.on_key_event({
        let window = window.as_weak();
        move |key_event| {
            let window = window.upgrade().unwrap();
            let mut parts = Vec::new();
            if key_event.modifiers.control {
                parts.push("Control");
            }
            if key_event.modifiers.alt {
                parts.push("Alt");
            }
            if key_event.modifiers.shift {
                parts.push("Shift");
            }
            if key_event.modifiers.meta {
                parts.push("Meta");
            }
            parts.push(&key_event.text);
            match Keys::from_parts(parts.iter().cloned()) {
                Ok(keys) => window.set_user_shortcut(keys),
                Err(err) => println!("Error parsing {parts:?} to Keys: {err}"),
            }
        }
    });

    println!();
    println!("Running Keys from Backend demo...");
    println!("Try pressing Ctrl+S, Ctrl+Z, F5, Ctrl+Plus");
    println!("The user shortcut has been set to Ctrl+P from Rust!");

    window.run().unwrap();
}
