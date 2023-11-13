use std::io::{Error, ErrorKind};

pub fn file_not_found() -> Error {
    Error::new(ErrorKind::NotFound, "File not found")
}

pub fn file_already_exists() -> Error {
    Error::new(ErrorKind::NotFound, "File not found")
}

pub fn dir_not_found() -> Error {
    Error::new(ErrorKind::NotFound, "Directory not found")
}

pub fn file_or_dir_not_found() -> Error {
    Error::new(ErrorKind::NotFound, "File or directory not found")
}

pub fn read_only_fs() -> Error {
    Error::new(ErrorKind::Other, "File system is read-only")
}