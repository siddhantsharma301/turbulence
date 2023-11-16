use std::collections::btree_map::Entry;
use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Result;
use std::path::PathBuf;

use crate::fs::error;
use crate::fs::open_options::OpenOptions;
use crate::world::World;

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
        let pb = PathBuf::from(&path);
        let path: Vec<_> = pb.as_path().iter().collect();
        let mut sys = &mut self.root;
        if path.len() > 1 {
            for i in 0..(path.len() - 1) {
                let component = path[i].to_str().unwrap().to_string();
                if !sys.contains_key(&component) {
                    sys.insert(
                        component.clone(),
                        FileSystemEntry::Directory(FileSystem::new()),
                    );
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
                    FileSystemEntry::File(_) => return Err(error::file_already_exists()),
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

        let pb = PathBuf::from(&path);
        let path: Vec<_> = pb.as_path().iter().collect();
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
                    FileSystemEntry::File(_) => return Err(error::file_already_exists()),
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
        let pb = PathBuf::from(&path);
        let path: Vec<_> = pb.as_path().iter().collect();
        let mut sys = self;
        for (i, component) in path.iter().enumerate() {
            let component = component.to_str().unwrap().to_string();
            let Some(child) = sys.root.get(&component) else {
                break;
            };

            if i + 1 == path.len() {
                return Ok(child.clone());
            }
            match child {
                FileSystemEntry::Directory(dir) => {
                    sys = dir;
                }
                FileSystemEntry::File(_) => return Err(error::file_not_found()),
            }
        }

        Err(error::file_or_dir_not_found())
    }

    pub fn update(&mut self, path: OsString, buffer: Vec<u8>) -> Result<()> {
        let mut entry = self.get(path)?;
        match entry {
            FileSystemEntry::Directory(_) => return Err(error::file_not_found()),
            FileSystemEntry::File(ref mut file) => {
                *file = buffer.clone();
            }
        }

        Ok(())
    }

    pub fn remove_dir(&mut self, path: OsString) -> Result<()> {
        if self.read_only {
            return Err(error::read_only_fs());
        }

        let pb = PathBuf::from(&path);
        let path: Vec<_> = pb.as_path().iter().collect();
        if path.len() == 0 {
            return Err(error::file_not_found());
        }
        let mut sys = &mut self.root;
        if path.len() > 1 {
            for i in 0..(path.len() - 1) {
                let component = path[i].to_str().unwrap().to_string();
                let child = match sys.get_mut(&component) {
                    Some(child) => child,
                    None => return Err(error::dir_not_found()),
                };
                match child {
                    FileSystemEntry::Directory(dir) => {
                        sys = &mut dir.root;
                    }
                    FileSystemEntry::File(_) => return Err(error::dir_not_found()),
                }
            }
        }

        let Some(name) = path.last() else {
            return Err(error::file_already_exists());
        };
        let name = name.to_str().unwrap().to_string();
        // TODO: Check this
        sys.remove(&name);

        Ok(())
    }

    pub fn has(&self, path: OsString) -> bool {
        let pb = PathBuf::from(&path);
        let path: Vec<_> = pb.as_path().iter().collect();
        let mut sys = self;
        for (i, component) in path.iter().enumerate() {
            let component = component.to_str().unwrap().to_string();
            let Some(child) = sys.root.get(&component) else {
                break;
            };

            if i + 1 == path.len() {
                return true;
            }
            match child {
                FileSystemEntry::Directory(dir) => {
                    sys = dir;
                }
                FileSystemEntry::File(_) => {}
            }
        }

        false
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn set_pwd(&mut self, pwd: OsString) {
        self.pwd = pwd;
    }

    pub fn pwd(self) -> String {
        self.pwd.to_str().unwrap().to_string()
    }
}

pub async fn read(path: OsString) -> Result<Vec<u8>> {
    let fs_entry = World::current(|world| world.current_host_mut().file_system.get(path))?;
    match fs_entry {
        FileSystemEntry::Directory(_) => Err(error::cannot_read_dir()),
        FileSystemEntry::File(data) => Ok(data),
    }
}

pub async fn try_exists(path: OsString) -> Result<bool> {
    Ok(World::current(|world| {
        world.current_host_mut().file_system.has(path)
    }))
}

// pub async fn write(path: OsString) -> Result<()> {
//     let mut file = OpenOptions::new().create(true).write(true).open(path).await?;
//     file.wri
// }
