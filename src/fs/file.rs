use std::ffi::OsString;

use super::open_options::OpenOptions;

pub struct File {
    buffer: Vec<u8>,
    cursor: usize,
    path: OsString,
    open_options: OpenOptions,
}

impl File {
    pub(crate) fn new(buffer: Vec<u8>, cursor: usize, path: OsString, open_options: OpenOptions) -> Self {
        Self {
            buffer,
            cursor,
            path,
            open_options
        }
    }


}
