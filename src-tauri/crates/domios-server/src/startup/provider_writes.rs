pub(crate) struct ProviderWritesComplete(());

pub(crate) fn complete(
    cli: Option<std::thread::JoinHandle<()>>,
    integrations: impl IntoIterator<Item = impl FnOnce()>,
    install_rules: impl FnOnce(),
) -> Result<ProviderWritesComplete, String> {
    if let Some(cli) = cli {
        cli.join()
            .map_err(|_| "startup CLI installation panicked".to_string())?;
    }
    for integration in integrations {
        integration();
    }
    install_rules();
    Ok(ProviderWritesComplete(()))
}

#[cfg(test)]
mod tests;
