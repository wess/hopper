use crate::machines::native::restore::{self, Owner, Prepared, Recovery};

pub(super) async fn run(
  owned: Owner,
  recovery: Recovery,
) -> anyhow::Result<(Prepared, Option<anyhow::Error>)> {
  (recovery.check)()?;
  let outcome = {
    let preparing = restore::prepare_retained(
      owned,
      recovery.boot.clone(),
      &recovery.target,
      &recovery.prior,
      recovery.predecessor,
    );
    tokio::pin!(preparing);
    loop {
      tokio::select! {
        result = &mut preparing => break result?,
        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => (recovery.check)()?,
      }
    }
  };
  (recovery.check)()?;
  let restore::Preparation { owner, result } = outcome;
  match result {
    Ok((boot, restore)) => Ok((
      Prepared {
        owner,
        boot,
        restore,
      },
      None,
    )),
    Err(failure) => {
      (recovery.check)()?;
      let recovered = restore::recover_preparation(owner, failure, recovery).await?;
      Ok((recovered.prepared, Some(recovered.failure)))
    }
  }
}
