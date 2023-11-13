use std::ffi::OsString;

use super::util::OpenOptions;

pub struct File {
    buffer: Vec<u8>,
    cursor: usize,
    path: OsString,
    open_options: OpenOptions,
}

