use std::io::Write;
use std::sync::mpsc;
use std::sync::Mutex;

pub struct WriteRequest {
    pub data: Vec<u8>,
}

pub struct WriteQueue {
    sender: Mutex<Option<mpsc::Sender<WriteRequest>>>,
}

impl WriteQueue {
    pub fn new(mut writer: Box<dyn Write + Send>) -> Self {
        let (sender, receiver) = mpsc::channel::<WriteRequest>();

        std::thread::spawn(move || {
            while let Ok(req) = receiver.recv() {
                if let Err(e) = writer.write_all(&req.data) {
                    log::warn!("PTY write failed: {}", e);
                    break;
                }
                if let Err(e) = writer.flush() {
                    log::warn!("PTY flush failed: {}", e);
                    break;
                }
            }
        });

        Self {
            sender: Mutex::new(Some(sender)),
        }
    }

    pub fn send(&self, data: Vec<u8>) -> Result<(), String> {
        let sender = self.sender.lock().map_err(|e| e.to_string())?;
        sender
            .as_ref()
            .ok_or_else(|| "PTY write channel closed".to_string())?
            .send(WriteRequest { data })
            .map_err(|_| "PTY write channel closed".to_string())
    }

    pub fn close(&self) {
        if let Ok(mut sender) = self.sender.lock() {
            let _ = sender.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct MockWriter {
        data: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for MockWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.data.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn send_and_receive() {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let writer = MockWriter {
            data: collected.clone(),
        };
        let queue = WriteQueue::new(Box::new(writer));

        queue.send(b"hello".to_vec()).unwrap();
        queue.send(b" world".to_vec()).unwrap();

        drop(queue);
        std::thread::sleep(std::time::Duration::from_millis(50));

        assert_eq!(&*collected.lock().unwrap(), b"hello world");
    }

    #[test]
    fn fifo_ordering() {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let writer = MockWriter {
            data: collected.clone(),
        };
        let queue = WriteQueue::new(Box::new(writer));

        for i in 0..10u8 {
            queue.send(vec![i]).unwrap();
        }
        drop(queue);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let data = collected.lock().unwrap();
        let expected: Vec<u8> = (0..10).collect();
        assert_eq!(&*data, &expected);
    }

    #[test]
    fn send_after_close_returns_error() {
        let collected = Arc::new(Mutex::new(Vec::new()));
        let _writer = MockWriter {
            data: collected.clone(),
        };
        let (sender, _receiver) = mpsc::channel::<WriteRequest>();
        let queue = WriteQueue {
            sender: Mutex::new(Some(sender)),
        };
        drop(_receiver);

        std::thread::sleep(std::time::Duration::from_millis(10));

        let result = queue.send(b"test".to_vec());
        assert!(result.is_err());
    }
}
