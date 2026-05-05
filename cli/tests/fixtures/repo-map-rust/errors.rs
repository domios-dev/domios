pub trait DescribeError {
    fn describe(&self) -> String;
}

pub enum AppError {
    Io(String),
    Db(String),
    Config(String),
}

impl DescribeError for AppError {
    fn describe(&self) -> String {
        match self {
            AppError::Io(s) | AppError::Db(s) | AppError::Config(s) => s.clone(),
        }
    }
}

pub fn wrap_io_error(msg: &str) -> AppError {
    AppError::Io(msg.to_string())
}
