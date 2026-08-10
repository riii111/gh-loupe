pub struct Exit {
    pub message: Option<String>,
    pub code: i32,
}

pub type Result<T> = std::result::Result<T, Exit>;

impl Exit {
    pub fn message(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            code: 1,
        }
    }

    pub fn child(code: i32, stderr: &[u8]) -> Self {
        Self {
            message: Some(String::from_utf8_lossy(stderr).trim_end().to_owned()),
            code,
        }
    }
}
