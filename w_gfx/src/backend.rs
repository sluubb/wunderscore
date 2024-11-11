use raw_window_handle::{DisplayHandle, WindowHandle};
use std::{error, fmt::Display};

pub mod vulkan;

pub trait Backend: Sized {
    fn new(display_handle: DisplayHandle) -> Result<Self, Error>;
    fn destroy(&mut self);
}

#[derive(Debug)]
pub struct Error {
    backend: &'static str,
    msg: String,
}
impl error::Error for Error {}
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("Graphics Error ({}): {}", self.backend, self.msg).as_str())?;
        Ok(())
    }
}
