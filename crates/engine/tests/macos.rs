#[path = "../src/machines/macos.rs"]
mod macos;

use macos::*;
use model::MachineInput;
#[test]
fn user_text_is_data_and_shortcuts_use_guest_key_codes() {
    let text = "\"; throw Error('injected');";
    let args = command(MachineInput::Text { text: text.into() }).unwrap();
    assert!(!args[4].contains(text));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&args[6]).unwrap()["text"],
        text
    );
    let args = command(MachineInput::Key {
        keys: "super+shift+a".into(),
    })
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&args[6]).unwrap();
    assert_eq!(value["code"], 0);
    assert_eq!(value["flags"], (1 << 20) | (1 << 17));
    assert!(command(MachineInput::Key {
        keys: "arbitrary+text".into()
    })
    .is_err());
}
