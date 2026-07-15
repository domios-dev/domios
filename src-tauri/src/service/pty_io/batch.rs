use crate::service::ServiceContext;

fn execute_pty_batch<E>(
    batch: Vec<super::SendBatchItem>,
    mut await_deadline: impl FnMut(std::time::Duration),
    mut submit: impl FnMut(&str, bool) -> Result<(), E>,
) -> Result<(), E> {
    for item in batch {
        if let Some(delay) = item.delay_ms {
            await_deadline(std::time::Duration::from_millis(delay));
        }
        submit(&item.text, item.enter)?;
    }
    Ok(())
}

impl ServiceContext {
    pub fn send_text_batch(
        &self,
        pane_id: &str,
        batch: Vec<super::SendBatchItem>,
    ) -> Result<(), String> {
        if let Some((plugin_id, session_id)) = self.pane_chat_backing(pane_id) {
            for item in batch {
                self.chatbackend_send(&plugin_id, &session_id, &item.text)?;
            }
            return Ok(());
        }
        self.ensure_pane_pty_for_send(pane_id)?;
        self.await_agent_boot_blocking(pane_id, std::time::Duration::from_secs(90))?;
        let pty_id = self.await_pty_link_blocking(pane_id, std::time::Duration::from_secs(15))?;
        execute_pty_batch(batch, super::await_action_deadline, |text, enter| {
            self.send_text_to_pty_smart(&pty_id, text, enter)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::inject_verify::InjectionError;

    #[test]
    fn batch_submit_failure_is_not_detached_or_erased() {
        let batch = vec![super::super::SendBatchItem {
            text: "parked".into(),
            enter: true,
            delay_ms: Some(5),
        }];
        let mut deadlines = Vec::new();
        let result = execute_pty_batch(
            batch,
            |deadline| deadlines.push(deadline),
            |_, _| {
                Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                    waited: std::time::Duration::ZERO,
                    screen_tail: "> parked".into(),
                })
            },
        );
        assert!(matches!(
            result,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
        assert_eq!(deadlines, [std::time::Duration::from_millis(5)]);
    }
}
