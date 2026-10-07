use super::{admit, open, Runtime};
use crate::bridge;
use gpui::App;
use host::VirtualLinuxInstallation;

pub fn watch(installation: VirtualLinuxInstallation, cx: &mut App) {
  let id = installation.id().to_owned();
  let name = installation.name().to_owned();
  let generation = installation.generation();
  cx.global_mut::<Runtime>()
    .watching
    .insert(id.clone(), generation);
  bridge::run(cx, installation.wait(), move |result, cx| {
    if !active(&id, generation, cx) {
      return;
    }
    let result = result.and_then(|installation| {
      let runtime = cx.global_mut::<Runtime>();
      let permit = installation.permit(&runtime.owner)?;
      if let Some(viewer) = runtime.viewers.get_mut(&id) {
        viewer.detach();
      }
      permit.retire(&mut runtime.owner)?;
      runtime.pending.insert(id.clone());
      Ok(permit)
    });
    let permit = match result {
      Ok(permit) => permit,
      Err(error) => {
        finish(&id, generation, Err(error), cx);
        return;
      }
    };
    bridge::run(cx, permit.prepare(), move |result, cx| {
      if !active(&id, generation, cx) {
        return;
      }
      let admission = match result.and_then(|prepared| admit(prepared, cx)) {
        Ok(admission) => admission,
        Err(error) => {
          finish(&id, generation, Err(error), cx);
          return;
        }
      };
      bridge::run(cx, admission.start(), move |result, cx| {
        if !active(&id, generation, cx) {
          return;
        }
        let result = result.and_then(|()| open(&id, &name, cx));
        finish(&id, generation, result, cx);
      });
    });
  });
}

fn active(id: &str, generation: u64, cx: &App) -> bool {
  cx.global::<Runtime>().watching.get(id) == Some(&generation)
}

fn finish(id: &str, generation: u64, result: anyhow::Result<()>, cx: &mut App) {
  if !active(id, generation, cx) {
    return;
  }
  let runtime = cx.global_mut::<Runtime>();
  runtime.watching.remove(id);
  runtime.pending.remove(id);
  if let Err(error) = result {
    runtime.messages.insert(id.into(), format!("{error:#}"));
  } else {
    runtime.messages.remove(id);
  }
}
