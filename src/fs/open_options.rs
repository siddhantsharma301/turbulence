use std::ffi::OsString;
use std::io::Result;

use crate::world::World;

use crate::fs::error;
use crate::fs::file::File;
use crate::fs::file_system::FileSystemEntry;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenOptions {
    pub read: bool,
    pub write: bool,
    pub append: bool,
    pub truncate: bool,
    pub create: bool,
    pub create_new: bool,
}

impl OpenOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&mut self, read: bool) -> &mut OpenOptions {
        self.read = read;
        self
    }

    pub fn write(&mut self, write: bool) -> &mut OpenOptions {
        self.write = write;
        self
    }

    pub fn append(&mut self, append: bool) -> &mut OpenOptions {
        self.append = append;
        self.write = append;
        self
    }

    pub fn truncate(&mut self, truncate: bool) -> &mut OpenOptions {
        self.truncate = truncate;
        self
    }

    pub fn create(&mut self, create: bool) -> &mut OpenOptions {
        self.create = create;
        self
    }

    pub fn create_new(&mut self, create_new: bool) -> &mut OpenOptions {
        self.create_new = create_new;
        self
    }

    pub async fn open(&self, path: OsString) -> Result<File> {
        if self.create_new {
            World::current(|world| {
                let host = world.current_host_mut();
                let fs = &mut host.file_system;
                if fs.has(path.clone()) {
                    fs.create_file(path.clone(), true)
                } else {
                    return Err(error::file_already_exists());
                }
            })?;
        } else if self.create {
            World::current(|world| {
                match world.current_host_mut().file_system.create_file(path.clone(), false) {
                    Err(_) => Err(error::file_already_exists()),
                    _ => Ok(())
                }
            })?;
        }

        let entry = World::current(|world| world.current_host_mut().file_system.get(path.clone()))?;
        let data = match entry {
            FileSystemEntry::Directory(_) => return Err(error::file_not_found()),
            FileSystemEntry::File(data) => data,
        };

        let cursor = if self.append { data.len() } else { 0 };

        Ok(File::new(data, cursor, path, self.clone()))
    }
}

#[cfg(test)]
mod test {
    use crate::Builder;
    use crate::fs::file::File;
    use crate::fs::file_system::FileSystemEntry;
    use crate::fs::open_options::OpenOptions;
    use crate::world::World;

    #[test]
    fn test_open() {
        let mut sim = Builder::new().build();
        sim.host("my_host", || async {
            let mut open_options = OpenOptions::new();
            let open = open_options.write(true).create(true);
            let r = open.open("test".to_string().into()).await;
            assert!(r.is_ok());

            let file = r.unwrap();
            let expected = File::new(Vec::new(), 0, "test".to_string().into(), open.clone());
            assert_eq!(file, expected);

            let r = World::current(|world| {
                world.current_host_mut().file_system.update("test".to_string().into(), "a".into())
            });
            assert!(r.is_ok());

            let r = World::current(|world| {
                world.current_host_mut().file_system.get("test".to_string().into())
            });
            assert!(r.is_ok());
            let file = r.unwrap();
            assert_eq!(file, FileSystemEntry::File("a".into()));

            Ok(())
        });

        let r = sim.run();
        assert!(r.is_ok())
    }
}
