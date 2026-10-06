use anyhow::{bail, Context};
use model::MachineInput;
use serde_json::json;

const SCRIPT: &str = r#"ObjC.import('CoreGraphics');
ObjC.bindFunction('CGPreflightPostEventAccess', ['bool', []]);
function run(argv) {
  const input = JSON.parse(argv[0]);
  if (!$.CGPreflightPostEventAccess()) {
    throw Error('Allow Accessibility for osascript inside the guest before sending input.');
  }
  if (input.type === 'text') {
    Application('System Events').keystroke(input.text);
    return;
  }
  if (input.type === 'key') {
    for (const down of [true, false]) {
      const event = $.CGEventCreateKeyboardEvent(null, input.code, down);
      $.CGEventSetFlags(event, input.flags);
      $.CGEventPost($.kCGHIDEventTap, event);
    }
    return;
  }
  const bounds = $.CGDisplayBounds($.CGMainDisplayID());
  const point = { x: bounds.origin.x + input.x * (bounds.size.width - 1) / 32767,
                  y: bounds.origin.y + input.y * (bounds.size.height - 1) / 32767 };
  $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent(null, $.kCGEventMouseMoved, point, 0));
  if (input.button !== null) {
    const kinds = [[1,2],[3,4],[25,26]][input.button];
    for (const kind of kinds) {
      $.CGEventPost($.kCGHIDEventTap, $.CGEventCreateMouseEvent(null, kind, point, input.button));
    }
  }
}"#;

pub fn command(input: MachineInput) -> anyhow::Result<Vec<String>> {
    let value = match input {
        MachineInput::Key { keys } => {
            let mut flags = 0_u32;
            let mut parts = keys.split('+').collect::<Vec<_>>();
            let key = parts
                .pop()
                .context("Provide a key or shortcut")?
                .to_ascii_lowercase();
            for modifier in parts {
                flags |= match modifier.to_ascii_lowercase().as_str() {
                    "shift" => 1 << 17,
                    "ctrl" | "control" => 1 << 18,
                    "alt" | "option" => 1 << 19,
                    "super" | "cmd" | "command" => 1 << 20,
                    _ => bail!("Unknown macOS modifier: {modifier}"),
                };
            }
            let code = match key.as_str() {
                "a" => 0,
                "s" => 1,
                "d" => 2,
                "f" => 3,
                "h" => 4,
                "g" => 5,
                "z" => 6,
                "x" => 7,
                "c" => 8,
                "v" => 9,
                "b" => 11,
                "q" => 12,
                "w" => 13,
                "e" => 14,
                "r" => 15,
                "y" => 16,
                "t" => 17,
                "1" => 18,
                "2" => 19,
                "3" => 20,
                "4" => 21,
                "6" => 22,
                "5" => 23,
                "equal" | "=" => 24,
                "9" => 25,
                "7" => 26,
                "minus" | "-" => 27,
                "8" => 28,
                "0" => 29,
                "bracketright" => 30,
                "o" => 31,
                "u" => 32,
                "bracketleft" => 33,
                "i" => 34,
                "p" => 35,
                "return" | "enter" => 36,
                "l" => 37,
                "j" => 38,
                "apostrophe" => 39,
                "k" => 40,
                "semicolon" => 41,
                "backslash" => 42,
                "comma" => 43,
                "slash" => 44,
                "n" => 45,
                "m" => 46,
                "period" => 47,
                "tab" => 48,
                "space" => 49,
                "grave" => 50,
                "backspace" => 51,
                "escape" | "esc" => 53,
                "f1" => 122,
                "f2" => 120,
                "f3" => 99,
                "f4" => 118,
                "f5" => 96,
                "f6" => 97,
                "f7" => 98,
                "f8" => 100,
                "f9" => 101,
                "f10" => 109,
                "f11" => 103,
                "f12" => 111,
                "home" => 115,
                "page_up" => 116,
                "delete" => 117,
                "end" => 119,
                "page_down" => 121,
                "left" => 123,
                "right" => 124,
                "down" => 125,
                "up" => 126,
                _ => bail!("Unknown macOS key: {key}"),
            };
            json!({"type":"key","code":code,"flags":flags})
        }
        MachineInput::Text { text } => {
            if text.len() > 16384 {
                bail!("Text input is limited to 16 KiB");
            }
            json!({"type":"text","text":text})
        }
        MachineInput::Pointer { x, y, button } => {
            if x > 32767 || y > 32767 {
                bail!("Pointer coordinates must be between 0 and 32767");
            }
            let button = match button.as_deref() {
                None => None,
                Some("left") => Some(0),
                Some("right") => Some(1),
                Some("middle") => Some(2),
                _ => bail!("Use left, middle, or right for the mouse button"),
            };
            json!({"type":"pointer","x":x,"y":y,"button":button})
        }
    };
    Ok(vec![
        "osascript".into(),
        "-l".into(),
        "JavaScript".into(),
        "-e".into(),
        SCRIPT.into(),
        "--".into(),
        value.to_string(),
    ])
}
