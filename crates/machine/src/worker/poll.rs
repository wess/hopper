use anyhow::{ensure, Context};
use machine::{
  devices::virtio::{gpu, input, pci},
  hypervisor::Memory,
  ipc,
  runtime::{Control, Mode},
  setup,
};
use model::native::{Command, InputDevice, Request, Response, Result as Reply, SetupStatus};
use std::{
  fs::File,
  time::{Duration, Instant},
};

pub(super) struct Parent {
  pub input: File,
  pub output: File,
  pub stream: ipc::Stream,
  pub pending: Option<Request>,
  pub held: bool,
  pub stopping: Option<u64>,
  pub input_since: Option<Instant>,
  pub write_since: Option<Instant>,
  pub setup: setup::Decoder,
}

impl Parent {
  pub fn reply(&mut self, id: u64, result: Reply, payload: &[u8]) -> anyhow::Result<()> {
    ipc::send(&mut self.stream, &Response { id, result }, payload)?;
    self.write_since = Some(Instant::now());
    Ok(())
  }

  pub fn poll(
    &mut self,
    devices: &mut [Option<pci::Device>; 7],
    ram: &mut Memory<'_>,
    mode: Mode,
  ) -> anyhow::Result<Control> {
    ensure!(
      devices
        .iter()
        .flatten()
        .all(|device| pci::fault(device).is_none()),
      "Native guest device failed"
    );
    let bytes = pci::receive_serial(devices[6].as_mut().context("Missing setup device")?, ram)?;
    if setup::status(&self.setup) != setup::Status::Invalid {
      for chunk in bytes.chunks(4096) {
        if setup::feed(&mut self.setup, chunk).is_err() {
          break;
        }
      }
    }
    if self
      .input_since
      .is_some_and(|since| since.elapsed() >= Duration::from_secs(5))
    {
      release(devices, ram)?;
      self.input_since = None;
    }
    if self.stream.flush(&mut self.output)? > 0 {
      self.write_since = Some(Instant::now());
    }
    if self.stream.pending() {
      ensure!(
        self
          .write_since
          .is_some_and(|since| since.elapsed() < Duration::from_secs(5)),
        "Native parent stopped receiving replies"
      );
      return Ok(if self.held {
        Control::Pause
      } else {
        Control::Continue
      });
    }
    let request = if let Some(request) = self.pending.take() {
      request
    } else {
      let Some(request) = ipc::receive(&mut self.stream, &mut self.input)? else {
        return Ok(if self.held {
          Control::Pause
        } else {
          Control::Continue
        });
      };
      request
    };
    if matches!(request.command, Command::Capture {} | Command::Pause {}) && mode != Mode::Paused {
      self.pending = Some(request);
      return Ok(Control::Pause);
    }
    let id = request.id;
    let result = match request.command {
      Command::Status {} => {
        let setup = match setup::status(&self.setup) {
          setup::Status::Waiting => SetupStatus::Waiting {},
          setup::Status::Active(phase) => SetupStatus::Active { phase },
          setup::Status::Failed(phase) => SetupStatus::Failed { phase },
          setup::Status::Deployed => SetupStatus::Deployed {},
          setup::Status::Invalid => SetupStatus::Invalid {},
        };
        self.reply(
          id,
          Reply::Status {
            paused: mode == Mode::Paused,
            setup,
          },
          &[],
        )
      }
      Command::Start { .. } => self.reply(
        id,
        Reply::Rejected {
          message: "Native worker is already initialized".into(),
        },
        &[],
      ),
      Command::Capture {} => {
        capture(devices, ram).and_then(|(header, bytes)| self.reply(id, header, &bytes))
      }
      Command::Pause {} => {
        self.held = true;
        self.reply(id, Reply::Paused {}, &[])
      }
      Command::Resume {} => {
        self.held = false;
        self.reply(id, Reply::Running {}, &[])
      }
      Command::Input { device, events } => {
        let update = || -> anyhow::Result<()> {
          ensure!(
            !events.is_empty() && events.len() <= 128,
            "Invalid native input batch"
          );
          let slot = match device {
            InputDevice::Keyboard => 2,
            InputDevice::Tablet => 3,
          };
          let events: Vec<_> = events
            .into_iter()
            .map(|event| input::Event {
              kind: event.kind,
              code: event.code,
              value: event.value,
            })
            .collect();
          let device = devices[slot].as_mut().context("Missing input device")?;
          pci::send_input(device, ram, &events)?;
          ensure!(pci::fault(device).is_none(), "Native input device failed");
          Ok(())
        };
        update().and_then(|()| {
          self.input_since = Some(Instant::now());
          self.reply(id, Reply::Accepted {}, &[])
        })
      }
      Command::Release {} => release(devices, ram).and_then(|()| {
        self.input_since = None;
        self.reply(id, Reply::Accepted {}, &[])
      }),
      Command::Stop {} => {
        release(devices, ram)?;
        self.stopping = Some(id);
        return Ok(Control::Stop);
      }
    };
    if result.is_err() {
      // malformed input and unavailable frames reject a command without exposing guest data.
      self.reply(
        id,
        Reply::Rejected {
          message: "Native command could not be applied".into(),
        },
        &[],
      )?;
    }
    Ok(if self.held {
      Control::Pause
    } else {
      Control::Continue
    })
  }
}

fn release(devices: &mut [Option<pci::Device>; 7], ram: &mut Memory<'_>) -> anyhow::Result<()> {
  for slot in [2, 3] {
    pci::release_input(devices[slot].as_mut().context("Missing input device")?, ram)?;
  }
  Ok(())
}

fn capture(
  devices: &mut [Option<pci::Device>; 7],
  ram: &Memory<'_>,
) -> anyhow::Result<(Reply, Vec<u8>)> {
  let device = devices[1].as_mut().context("Missing native display")?;
  pci::refresh(device, ram)?;
  let frame = pci::display(device)
    .and_then(gpu::frame)
    .context("Guest has no display frame")?;
  ensure!(
    ipc::frame_size(frame.width, frame.height)? == frame.rgba.len(),
    "Invalid guest frame"
  );
  Ok((
    Reply::Frame {
      width: frame.width,
      height: frame.height,
      generation: frame.generation,
    },
    frame.rgba.clone(),
  ))
}
