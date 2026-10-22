use crate::service::ServiceContext;

fn execute_pty_batch<T, E>(
    batch: Vec<(T, Option<u64>)>,
    mut await_deadline: impl FnMut(std::time::Duration),
    mut submit: impl FnMut(&T) -> Result<(), E>,
) -> Result<(), E> {
    for (input, delay) in batch {
        if let Some(delay) = delay {
            await_deadline(std::time::Duration::from_millis(delay));
        }
        submit(&input)?;
    }
    Ok(())
}

impl ServiceContext {
    pub fn send_text_batch(
        &self,
        tab_id: &str,
        batch: Vec<super::SendBatchItem>,
    ) -> Result<(), String> {
        let batch = batch
            .into_iter()
            .map(|item| (self.programmatic_input(tab_id, &item.text), item.delay_ms))
            .collect();
        self.send_classified_batch(tab_id, batch)
    }

    pub fn send_classified_batch(
        &self,
        tab_id: &str,
        batch: Vec<(crate::service::messenger::AgentInput, Option<u64>)>,
    ) -> Result<(), String> {
        execute_pty_batch(batch, super::await_action_deadline, |input| {
            self.send_text(tab_id, input)
        })
    }
}

#[cfg(test)]
mod tests;
