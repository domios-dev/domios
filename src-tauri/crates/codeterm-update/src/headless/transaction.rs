use super::{Error, ErrorCode};

pub trait InstallPort {
    fn download(&mut self) -> Result<(), Error>;
    fn verify(&mut self) -> Result<(), Error>;
    fn stage(&mut self) -> Result<(), Error>;
    fn swap(&mut self) -> Result<(), Error>;
    fn restart(&mut self) -> Result<(), Error>;
    fn health(&mut self) -> Result<(), Error>;
    fn rollback(&mut self) -> Result<(), Error>;
    fn restart_previous(&mut self) -> Result<(), Error>;
    fn health_previous(&mut self) -> Result<(), Error>;
    fn commit(&mut self) -> Result<(), Error>;
}

pub fn install(port: &mut impl InstallPort) -> Result<(), Error> {
    port.download()?;
    port.verify()?;
    port.stage()?;
    port.swap()?;
    if let Err(failure) = port
        .restart()
        .and_then(|()| port.health())
        .and_then(|()| port.commit())
    {
        restore(port).map_err(|rollback| {
            Error::new(
                ErrorCode::RollbackFailed,
                format!("Activation failed ({failure}); recovery failed ({rollback})"),
            )
        })?;
        return Err(failure);
    }
    Ok(())
}

pub fn restore(port: &mut impl InstallPort) -> Result<(), Error> {
    port.rollback()?;
    port.restart_previous()?;
    port.health_previous()
}

#[cfg(test)]
mod tests;
