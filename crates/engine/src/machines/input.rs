use super::{Actor, Machines};
use anyhow::bail;
use model::{GuestOs, MachineInput};
use serde_json::json;

impl Machines {
    pub async fn input(&self, id: &str, actor: Actor, input: MachineInput) -> anyhow::Result<()> {
        let machine = self.machine(id, actor)?;
        if machine.guest == GuestOs::Windows {
            let _lock = self.lock(id)?;
            self.machine(id, actor)?;
            let events = match input {
                MachineInput::Key {keys} => super::qmp::key_events(&keys)?,
                MachineInput::Pointer {x,y,button} => {
                    if x>32767 || y>32767 { bail!("Pointer coordinates must be between 0 and 32767"); }
                    let mut events=vec![json!({"type":"abs","data":{"axis":"x","value":x}}),json!({"type":"abs","data":{"axis":"y","value":y}})];
                    if let Some(button)=button {
                        if !["left","right","middle"].contains(&button.as_str()) { bail!("Use left, middle, or right for the mouse button"); }
                        for down in [true,false] {events.push(json!({"type":"btn","data":{"down":down,"button":button}}));}
                    }
                    events
                }
                MachineInput::Text {..} => bail!("Windows text input requires a guest command; use vm.exec with PowerShell, or vm.input with individual QEMU key names"),
            };
            return super::qmp::input(&self.root.join("lima").join(id).join("qmp.sock"), events)
                .await;
        }
        let args = match (machine.guest, input) {
            (GuestOs::Linux, input) => {
                let mut args = vec![
                    "sh".into(),
                    "-c".into(),
                    "export DISPLAY=:0 XAUTHORITY=\"$HOME/.Xauthority\"; exec xdotool \"$@\""
                        .into(),
                    "hopper-input".into(),
                ];
                match input {
                    MachineInput::Key { keys } => {
                        if keys.is_empty() || keys.len() > 256 {
                            bail!("Provide a key or shortcut of at most 256 characters");
                        }
                        args.extend(["key".into(), "--clearmodifiers".into(), "--".into(), keys]);
                    }
                    MachineInput::Text { text } => {
                        if text.len() > 16384 {
                            bail!("Text input is limited to 16 KiB");
                        }
                        args.extend(["type".into(), "--clearmodifiers".into(), "--".into(), text]);
                    }
                    MachineInput::Pointer { x, y, button } => {
                        if x > 32767 || y > 32767 {
                            bail!("Pointer coordinates must be between 0 and 32767");
                        }
                        let button = match button.as_deref() {
                            None => "0",
                            Some("left") => "1",
                            Some("middle") => "2",
                            Some("right") => "3",
                            _ => bail!("Use left, middle, or right for the mouse button"),
                        };
                        args = vec!["sh".into(),"-c".into(),r#"export DISPLAY=:0 XAUTHORITY="$HOME/.Xauthority"; geometry=$(xdotool getdisplaygeometry); width=${geometry% *}; height=${geometry#* }; x=$(( $1 * (width-1) / 32767 )); y=$(( $2 * (height-1) / 32767 )); xdotool mousemove "$x" "$y"; if [ "$3" != 0 ]; then xdotool click "$3"; fi"#.into(),"hopper-pointer".into(),x.to_string(),y.to_string(),button.into()];
                    }
                }
                args
            }
            (GuestOs::Macos, input) => super::macos::command(input)?,
            (GuestOs::Windows, _) => bail!("Windows input automation is not implemented yet"),
        };
        self.exec(id, actor, &args).await?;
        Ok(())
    }
}
