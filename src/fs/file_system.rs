use std::collections::HashMap;
use std::ffi::OsString;
use std::io::Result;
use std::path::PathBuf;

use tokio::io::AsyncWriteExt;

use crate::fs::error;
use crate::fs::open_options::OpenOptions;
use crate::world::World;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileSystem {
    root: HashMap<String, FileSystemEntry>,
    pwd: OsString,
    read_only: bool,
}

#[derive(Clone, Debug, PartialEq)]
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

    pub fn append(&mut self, path: OsString, buffer: Vec<u8>) -> Result<()> {
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

        let Some(file_name) = path.last() else {
            return Err(error::file_already_exists());
        };
        let file_name = file_name.to_str().unwrap().to_string();
        if let Some(entry) = sys.get_mut(&file_name) {
            if let FileSystemEntry::File(data) = entry {
                data.extend(buffer);
                return Ok(());
            }
        }

        Ok(())
    }

    pub fn update(&mut self, path: OsString, buffer: Vec<u8>) -> Result<()> {
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

        let Some(file_name) = path.last() else {
            return Err(error::file_already_exists());
        };
        let file_name = file_name.to_str().unwrap().to_string();
        if let Some(entry) = sys.get_mut(&file_name) {
            if let FileSystemEntry::File(data) = entry {
                *data = buffer;
                return Ok(());
            }
        }

        Err(error::file_not_found())
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
        sys.remove(&name);

        Ok(())
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

pub async fn append(path: OsString, buffer: Vec<u8>) -> Result<()> {
    let _ = OpenOptions::new()
        .write(true)
        .create(true)
        .append(true)
        .open(path.clone())
        .await?;
    World::current(|world| world.current_host_mut().file_system.append(path, buffer))
}

pub async fn copy(from: OsString, to: OsString) -> Result<()> {
    let data = read(from).await?;
    write(to, data).await
}

pub async fn create_file(path: OsString, replace: bool) -> Result<()> {
    World::current(|world| {
        world
            .current_host_mut()
            .file_system
            .create_file(path, replace)
    })
}

pub async fn create_dir(path: OsString) -> Result<()> {
    World::current(|world| world.current_host_mut().file_system.create_dir(path))
}

pub async fn try_exists(path: OsString) -> Result<bool> {
    Ok(World::current(|world| {
        world.current_host_mut().file_system.has(path)
    }))
}

pub async fn read(path: OsString) -> Result<Vec<u8>> {
    let fs_entry = World::current(|world| world.current_host_mut().file_system.get(path))?;
    match fs_entry {
        FileSystemEntry::Directory(_) => Err(error::cannot_read_dir()),
        FileSystemEntry::File(data) => Ok(data),
    }
}

pub async fn remove_dir(path: OsString) -> Result<()> {
    World::current(|world| world.current_host_mut().file_system.remove_dir(path))
}

pub async fn write(path: OsString, buffer: Vec<u8>) -> Result<()> {
    let _ = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path.clone())
        .await?;
    // TODO: Fix this function
    World::current(|world| world.current_host_mut().file_system.update(path, buffer))?;
    Ok(())
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use crate::fs::file_system::{FileSystem, FileSystemEntry};

    #[test]
    fn test_default() {
        let fs = FileSystem::default();
        let expected = FileSystem {
            root: HashMap::new(),
            pwd: "".into(),
            read_only: false,
        };
        assert_eq!(fs, expected);
    }

    #[test]
    fn test_create_dir() {
        let mut fs = FileSystem::default();

        let dir_name: OsString = "test".to_string().into();
        let r = fs.create_dir(dir_name.clone());
        let expected = (
            dir_name.into_string().unwrap(),
            FileSystemEntry::Directory(FileSystem::default()),
        );
        assert!(r.is_ok());
        assert_eq!(fs.root, HashMap::from([expected]));

        let mut fs = FileSystem::default();
        fs.read_only = true;
        let dir_name: OsString = "test".to_string().into();
        let r = fs.create_dir(dir_name.clone());
        assert!(r.is_err());
        assert_eq!(fs.root, HashMap::new());

        let mut fs = FileSystem::default();
        let dir_name: OsString = "test/test".to_string().into();
        let r = fs.create_dir(dir_name.clone());
        let expected = (
            "test".to_string(),
            FileSystemEntry::Directory(FileSystem {
                root: HashMap::from([(
                    "test".to_string().into(),
                    FileSystemEntry::Directory(FileSystem::default()),
                )]),
                pwd: "".to_string().into(),
                read_only: false,
            }),
        );
        assert!(r.is_ok());
        assert_eq!(fs.root, HashMap::from([expected]));
    }

    #[test]
    fn test_create_file() {
        let mut fs = FileSystem::default();

        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        let expected = (
            file_name.clone().into_string().unwrap(),
            FileSystemEntry::File(Vec::new()),
        );
        assert!(r.is_ok());
        assert_eq!(fs.root, HashMap::from([expected]));

        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());
        let expected = (
            file_name.into_string().unwrap(),
            FileSystemEntry::File(Vec::new()),
        );
        assert_eq!(fs.root, HashMap::from([expected]));
    }

    #[test]
    fn test_create_dir_and_file() {
        let mut fs = FileSystem::default();

        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());
        let dir_name: OsString = "test".to_string().into();
        let r = fs.create_dir(dir_name.clone());
        assert!(r.is_ok());

        let expected = [
            (
                file_name.into_string().unwrap(),
                FileSystemEntry::File(Vec::new()),
            ),
            (
                dir_name.into_string().unwrap(),
                FileSystemEntry::Directory(FileSystem::default()),
            ),
        ];

        assert_eq!(fs.root, HashMap::from(expected));
    }

    #[test]
    fn test_has() {
        let mut fs = FileSystem::default();
        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());

        let fetched = fs.has(file_name.clone());
        assert!(fetched);
        let fetched = fs.has("no".to_string().into());
        assert!(!fetched);
    }

    #[test]
    fn test_get() {
        let mut fs = FileSystem::default();
        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());

        let fetched = fs.get(file_name);
        assert!(fetched.is_ok());
        let file = fetched.unwrap();
        let expected = FileSystemEntry::File(Vec::new());
        assert_eq!(file, expected);

        let fetched = fs.get("no".to_string().into());
        assert!(fetched.is_err());
    }

    #[test]
    fn test_append() {
        let mut fs = FileSystem::default();

        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());

        let mut buf = Vec::from([1, 2, 3]);
        let r = fs.update(file_name.clone(), buf.clone());
        assert!(r.is_ok());
        let r = fs.append(file_name.clone(), buf.clone());
        assert!(r.is_ok());
        let fetched = fs.get(file_name);
        assert!(fetched.is_ok());
        let file = fetched.unwrap();
        buf.extend(buf.clone());
        println!("file is {:?}, buf is {:?}", file.clone(), buf.clone());
        assert_eq!(file, FileSystemEntry::File(buf));
    }

    #[test]
    fn test_update() {
        let mut fs = FileSystem::default();

        let file_name: OsString = "test.txt".to_string().into();
        let r = fs.create_file(file_name.clone(), false);
        assert!(r.is_ok());

        let buf = Vec::from([1, 2, 3]);
        let r = fs.update(file_name.clone(), buf.clone());
        assert!(r.is_ok());
        let fetched = fs.get(file_name);
        assert!(fetched.is_ok());
        let file = fetched.unwrap();
        assert_eq!(file, FileSystemEntry::File(buf));
    }
}
