use crate::ports::transport::Input;
use crate::service::ServiceContext;

impl ServiceContext {
    pub(crate) fn deliver(&self, tab_id: &str, input: Input<'_>) -> Result<(), String> {
        match input {
            Input::Text(text) => self.send_text(tab_id, text),
            Input::TextDetached(text) => {
                self.send_text_detached(tab_id, text);
                Ok(())
            }
            Input::Insert(text) => self.insert_text(tab_id, text),
            Input::Raw(bytes) => self.write_raw(tab_id, bytes),
            Input::Batch(items) => self.send_text_batch(tab_id, items),
        }
    }
}
