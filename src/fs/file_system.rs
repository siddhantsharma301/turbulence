use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Result;
use std::path::PathBuf;

use super::error;

#[derive(Clone, Debug, Default)]
pub struct FileSystem {
    root: HashMap<String, FileSystemEntry>,
    pwd: OsString,
    read_only: bool,
}

#[derive(Clone, Debug)]
pub enum FileSystemEntry {
    Directory(FileSystem),
    File(Vec<u8>),
}

impl FileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn create_dir(&mut self, path: OsString) -> Result<()> {
        if self.read_only {
            return Err(error::read_only_fs());
        }

        let path: Vec<_> = PathBuf::from(path).as_path().iter().collect();
        let mut sys = &mut self.root;
        if path.len() > 1 {
            for i in 0..(path.len() - 1) {
                let component = path[i].to_str().unwrap().to_string();
                if !sys.contains_key(&component) {
                    sys.insert(component, FileSystemEntry::Directory(FileSystem::new()));
                }

                let child = match sys.get_mut(&component) {
                    Some(child) => child,
                    None => {
                        return Err(error::dir_not_found());
                    }
                };
                match child {
                    FileSystemEntry::Directory(dir) => {
                        sys = &mut dir.root;
                    }
                    FileSystemEntry::File(_) => {
                        return Err(error::file_already_exists());
                    }
                }
            }
        }

        let Some(dir_name) = path.last() else {
            return Ok(());
        };
        let dir_name = dir_name.to_str().unwrap().to_string();
        if sys.contains_key(&dir_name) {
            return Err(error::file_already_exists());
        }

        sys.insert(dir_name, FileSystemEntry::Directory(FileSystem::default()));

        Ok(())
    }

    pub fn create_file(&mut self, path: OsString, replace: bool) -> Result<()> {
        if self.read_only {
            return Err(error::read_only_fs());
        }

        let path: Vec<_> = PathBuf::from(path).as_path().iter().collect();
        if path.len() == 0 {
            return Err(error::file_not_found());
        }
        let mut sys = &mut self.root;
        if path.len() > 1 {
            for i in 0..(path.len() - 1) {
                let component = path[i].to_str().unwrap().to_string();

                let child = match sys.get_mut(&component) {
                    Some(child) => child,
                    None => {
                        return Err(error::dir_not_found());
                    }
                };
                match child {
                    FileSystemEntry::Directory(dir) => {
                        sys = &mut dir.root;
                    }
                    FileSystemEntry::File(_) => {
                        return Err(error::file_already_exists());
                    }
                }
            }
        }

        let Some(file_name) = path.last() else {
            return Err(error::file_already_exists());
        };
        let file_name = file_name.to_str().unwrap().to_string();
        if !sys.contains_key(&file_name) || replace {
            sys.insert(file_name, FileSystemEntry::File(Vec::new()));
        }

        Ok(())
    }

    pub fn get(&self, path: OsString) -> Result<FileSystemEntry> {
        let path: Vec<_> = PathBuf::from(path).as_path().iter().collect();
        let mut sys = &mut self.root;
        for i in 0..path.len() {
            let component = path[i].to_str().unwrap().to_string();
            let Some(child) = sys.get(&component) else {
                break;
            };
            if i + 1 == path.len() {
                return Ok(child.clone());
            }
            match child {
                FileSystemEntry::Directory(dir) => {
                    sys = &mut dir.root;
                }
                FileSystemEntry::File(_) => {
                    return Err(error::file_not_found());
                }
            }
        }

        Err(error::file_or_dir_not_found())
    }

    pub fn has(&self, path: OsString) -> bool {
        let path: Vec<_> = PathBuf::from(path).as_path().iter().collect();
        let mut sys = &mut self.root;
        for i in 0..path.len() {
            let component = path[i].to_str().unwrap().to_string();
            let Some(child) = sys.get(&component) else {
                break;
            };
            if i + 1 == path.len() {
                return true;
            }

            match child {
                FileSystemEntry::Directory(dir) => {
                    sys = &mut dir.root;
                }
                FileSystemEntry::File(_) => {}
            }
        }

        false
    }
    
}
