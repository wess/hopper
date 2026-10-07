use crate::bridge;
use host::{Host, MachineActor, MachineInputLease};
use model::native::{Command, InputDevice, InputEvent};
use std::sync::{
  atomic::{AtomicU64, Ordering},
  Arc,
};
use tokio::sync::{mpsc, watch};

pub(super) struct Input {
  queue: mpsc::Sender<(u64, Command)>,
  epoch: Arc<AtomicU64>,
  release: watch::Sender<u64>,
  pub error: watch::Receiver<Option<String>>,
}

pub(super) fn create(host: Arc<Host>, id: String) -> Input {
  let (queue, mut incoming) = mpsc::channel::<(u64, Command)>(32);
  let (release, mut released) = watch::channel(0u64);
  let epoch = Arc::new(AtomicU64::new(0));
  let generation = epoch.clone();
  let (errors, error) = watch::channel(None);
  bridge::runtime().spawn(async move {
    let mut owner = None;
    loop {
      tokio::select! {
        biased;
        result = released.changed() => {
          close(&mut owner, &errors).await;
          if result.is_err() { break; }
        }
        item = incoming.recv() => {
          let Some((epoch, command)) = item else {
            close(&mut owner, &errors).await;
            break;
          };
          if epoch != generation.load(Ordering::Acquire) { continue; }
          let result = tokio::select! {
            biased;
            changed = released.changed() => {
              close(&mut owner, &errors).await;
              if changed.is_err() { break; }
              continue;
            }
            result = dispatch(&mut owner, &host, &id, command) => result,
          };
          match result {
            Ok(_) => { errors.send_replace(None); }
            Err(error) => {
              generation.fetch_add(1, Ordering::AcqRel);
              errors.send_replace(Some(format!("Guest input failed: {error:#}")));
              close(&mut owner, &errors).await;
            }
          }
        }
      }
    }
  });
  Input {
    queue,
    epoch,
    release,
    error,
  }
}

async fn close(owner: &mut Option<MachineInputLease>, errors: &watch::Sender<Option<String>>) {
  if let Some(owner) = owner.take() {
    if let Err(error) = owner.close().await {
      errors.send_replace(Some(format!("Guest input release failed: {error:#}")));
    }
  }
}

async fn dispatch(
  owner: &mut Option<MachineInputLease>,
  host: &Host,
  id: &str,
  command: Command,
) -> anyhow::Result<model::native::Result> {
  if owner.as_ref().is_some_and(|owner| !owner.is_active()) {
    owner.take().unwrap().close().await?;
  }
  if owner.is_none() {
    *owner = Some(
      host
        .native_machines()
        .acquire_input(id, MachineActor::Person)
        .await?,
    );
  }
  owner.as_ref().unwrap().send(command).await
}

impl Input {
  pub fn send(&self, command: Command) -> bool {
    if self
      .queue
      .try_send((self.epoch.load(Ordering::Acquire), command))
      .is_ok()
    {
      return true;
    }
    self.release();
    false
  }

  pub fn release(&self) {
    let epoch = self.epoch.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    self.release.send_replace(epoch);
  }
}

pub(super) fn events(device: InputDevice, mut events: Vec<InputEvent>) -> Command {
  events.push(InputEvent {
    kind: 0,
    code: 0,
    value: 0,
  });
  Command::Input { device, events }
}

pub(super) fn key(code: u16, value: i32) -> InputEvent {
  InputEvent {
    kind: 1,
    code,
    value,
  }
}

// GPUI exposes logical names; guest layout and text composition remain guest-tool work.
pub(super) fn code(key: &str) -> Option<u16> {
  Some(match key {
    "escape" => 1,
    "1" | "!" => 2,
    "2" | "@" => 3,
    "3" | "#" => 4,
    "4" | "$" => 5,
    "5" | "%" => 6,
    "6" | "^" => 7,
    "7" | "&" => 8,
    "8" | "*" => 9,
    "9" | "(" => 10,
    "0" | ")" => 11,
    "-" | "_" => 12,
    "=" | "+" => 13,
    "backspace" => 14,
    "tab" => 15,
    "q" => 16,
    "w" => 17,
    "e" => 18,
    "r" => 19,
    "t" => 20,
    "y" => 21,
    "u" => 22,
    "i" => 23,
    "o" => 24,
    "p" => 25,
    "[" | "{" => 26,
    "]" | "}" => 27,
    "enter" => 28,
    "a" => 30,
    "s" => 31,
    "d" => 32,
    "f" => 33,
    "g" => 34,
    "h" => 35,
    "j" => 36,
    "k" => 37,
    "l" => 38,
    ";" | ":" => 39,
    "'" | "\"" => 40,
    "`" | "~" => 41,
    "\\" | "|" => 43,
    "z" => 44,
    "x" => 45,
    "c" => 46,
    "v" => 47,
    "b" => 48,
    "n" => 49,
    "m" => 50,
    "," | "<" => 51,
    "." | ">" => 52,
    "/" | "?" => 53,
    "space" | " " => 57,
    "capslock" => 58,
    "f1" => 59,
    "f2" => 60,
    "f3" => 61,
    "f4" => 62,
    "f5" => 63,
    "f6" => 64,
    "f7" => 65,
    "f8" => 66,
    "f9" => 67,
    "f10" => 68,
    "f11" => 87,
    "f12" => 88,
    "home" => 102,
    "up" => 103,
    "pageup" => 104,
    "left" => 105,
    "right" => 106,
    "end" => 107,
    "down" => 108,
    "pagedown" => 109,
    "insert" => 110,
    "delete" => 111,
    _ => return None,
  })
}

pub(super) fn position(view: [f32; 4], image: [f32; 2], pointer: [f32; 2]) -> Option<[i32; 2]> {
  if view
    .into_iter()
    .chain(image)
    .chain(pointer)
    .any(|value| !value.is_finite())
    || view[2] <= 0.0
    || view[3] <= 0.0
    || image[0] <= 0.0
    || image[1] <= 0.0
  {
    return None;
  }
  let scale = (view[2] / image[0]).min(view[3] / image[1]);
  let display = [image[0] * scale, image[1] * scale];
  let origin = [
    view[0] + (view[2] - display[0]) / 2.0,
    view[1] + (view[3] - display[1]) / 2.0,
  ];
  let relative = [pointer[0] - origin[0], pointer[1] - origin[1]];
  if relative[0] < 0.0 || relative[1] < 0.0 || relative[0] > display[0] || relative[1] > display[1]
  {
    return None;
  }
  Some([
    (relative[0] / display[0] * 65535.0).round() as i32,
    (relative[1] / display[1] * 65535.0).round() as i32,
  ])
}
