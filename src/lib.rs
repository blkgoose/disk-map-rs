use std::collections::hash_map::RandomState;
use std::fmt::Display;
use std::fs::remove_dir_all;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::{create_dir_all, read_dir, remove_file};
use std::marker::PhantomData;
use std::path::PathBuf;

use advisory_lock::{AdvisoryFileLock, FileLockMode};
use serde::de::DeserializeOwned;
use serde::Serialize;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct DiskMap<K, V> {
    directory: PathBuf,
    phantom: PhantomData<fn() -> (K, V)>,
    hasher: RandomState,
}

#[derive(Debug, Clone)]
pub enum Error {
    CannotOpenDirectory,
    CannotOpenFile,
    CannotReadFromFile,
    CannotInsert,
    CannotAlterFile,
    CannotDeleteFile,
    CannotGetLock,
}

#[allow(dead_code)]
impl<K, V> DiskMap<K, V>
where
    K: Serialize + DeserializeOwned,
    K: Display + From<String>,
    K: PartialEq,
    K: Clone,
    V: Serialize + DeserializeOwned,
{
    fn filename(&self, key: &K) -> PathBuf {
        PathBuf::from(format!(
            "{}/{}",
            &self.directory.to_str().unwrap().to_string(),
            key
        ))
    }

    pub fn open_new(directory: &str) -> Result<DiskMap<K, V>, Error> {
        remove_dir_all(directory).ok();

        Self::open(directory)
    }

    pub fn open(directory: &str) -> Result<DiskMap<K, V>, Error> {
        match create_dir_all(&directory) {
            Ok(()) => Ok(DiskMap {
                directory: PathBuf::from(directory.to_string()),
                phantom: PhantomData,
                hasher: RandomState::new(),
            }),
            Err(_) => Err(Error::CannotOpenDirectory),
        }
    }

    pub fn insert(&self, key: K, value: V) -> Result<(), Error> {
        let fname = self.filename(&key);

        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .append(true)
            .open(fname);

        match file {
            Err(_) => Err(Error::CannotOpenFile),
            Ok(f) => {
                f.lock(FileLockMode::Exclusive).unwrap();
                match serde_cbor::to_writer(f, &value) {
                    Err(_) => Err(Error::CannotInsert),
                    Ok(v) => Ok(v),
                }
            }
        }
    }

    pub fn get(&self, key: &K) -> Result<V, Error> {
        let fname = self.filename(&key);

        match File::open(fname) {
            Err(_) => Err(Error::CannotOpenFile),
            Ok(f) => match f.lock(FileLockMode::Shared) {
                Err(_) => Err(Error::CannotGetLock),
                Ok(_) => match serde_cbor::from_reader(f) {
                    Err(_) => Err(Error::CannotReadFromFile),
                    Ok(v) => Ok(v),
                },
            },
        }
    }

    pub fn alter(&self, key: &K, mut alter_function: impl FnMut(V) -> V) -> Result<(), Error> {
        let v = self.get(&key)?;

        let fname = self.filename(&key);

        let lfile = File::open(&fname).unwrap();
        lfile.lock(FileLockMode::Exclusive).ok();

        let file = OpenOptions::new().write(true).truncate(true).open(fname);

        match file {
            Err(_) => Err(Error::CannotOpenFile),
            Ok(f) => match serde_cbor::to_writer(f, &alter_function(v)) {
                Err(_) => Err(Error::CannotAlterFile),
                Ok(v) => Ok(v),
            },
        }
    }

    pub fn delete(&self, key: &K) -> Result<(), Error> {
        let fname = self.filename(&key);

        match remove_file(&fname) {
            Err(_) => Err(Error::CannotDeleteFile),
            Ok(_) => Ok(()),
        }
    }

    pub fn overwrite(&self, key: K, value: V) -> Result<(), Error> {
        let fname = self.filename(&key);

        let lfile = File::open(&fname).unwrap();
        lfile.lock(FileLockMode::Exclusive).ok();

        let file = OpenOptions::new().write(true).truncate(true).open(fname);

        match file {
            Err(_) => Err(Error::CannotOpenFile),
            Ok(f) => match serde_cbor::to_writer(f, &value) {
                Err(_) => Err(Error::CannotAlterFile),
                Ok(_) => Ok(()),
            },
        }
    }

    pub fn get_keys(&self) -> Result<Vec<K>, Error> {
        match read_dir(&self.directory) {
            Ok(c) => {
                let files: Vec<String> = c
                    .into_iter()
                    .filter(|r| r.is_ok())
                    .map(|r| r.unwrap().path())
                    .map(|r| r.file_name().unwrap().to_owned().into_string().unwrap())
                    .collect();

                let casted: Vec<K> = files.into_iter().map(|r| r.into()).collect();

                Ok(casted)
            }
            Err(_) => Err(Error::CannotOpenDirectory),
        }
    }

    pub fn contains_key(&self, key: &K) -> Result<bool, Error> {
        match self.get_keys() {
            Err(e) => Err(e),
            Ok(keys) => Ok(keys.contains(key)),
        }
    }

    pub fn len(&self) -> Result<usize, Error> {
        match self.get_keys() {
            Err(e) => Err(e),
            Ok(keys) => Ok(keys.len()),
        }
    }

    pub fn as_vec(&self) -> Result<Vec<(K, V)>, Error> {
        match self.get_keys() {
            Err(e) => Err(e),
            Ok(keys) => {
                let mut v = vec![];

                for key in keys {
                    let val = self.get(&key)?;

                    v.push((key, val));
                }

                Ok(v)
            }
        }
    }

    pub fn clear(&self) -> Result<(), Error> {
        match self.get_keys() {
            Err(e) => Err(e),
            Ok(keys) => {
                for key in keys.iter() {
                    self.delete(key)?;
                }
                Ok(())
            }
        }
    }

    pub fn alter_with_default(
        &self,
        key: &K,
        default: V,
        alter_function: impl FnMut(V) -> V,
    ) -> Result<(), Error> {
        if !self.contains_key(key)? {
            self.insert(key.clone(), default)?;
        }

        self.alter(key, alter_function)
    }
}

#[cfg(test)]
mod test {
    use std::{collections::HashSet, thread, time::Duration};

    use serde::{Deserialize, Serialize};

    use super::*;

    #[test]
    fn insert_and_get() {
        let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/test_db_1").unwrap();

        d.insert("a".to_owned(), 1).unwrap();
        d.insert("b".to_owned(), 2).unwrap();
        d.insert("c".to_owned(), 3).unwrap();

        let v = d.get(&"a".to_owned()).unwrap();

        assert_eq!(v, 1);
    }

    #[test]
    fn alter() {
        let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/test_db_2").unwrap();

        d.insert("a".to_owned(), 12000).unwrap();
        d.insert("b".to_owned(), 2).unwrap();
        d.insert("c".to_owned(), 3).unwrap();

        d.alter(&"a".to_owned(), |_| 1).unwrap();

        let v = d.get(&"a".to_owned()).unwrap();

        assert_eq!(v, 1);
    }

    #[test]
    fn delete() {
        let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/test_db_3").unwrap();

        d.insert("a".to_owned(), 1).unwrap();
        d.insert("b".to_owned(), 2).unwrap();
        d.insert("c".to_owned(), 3).unwrap();

        d.delete(&"a".to_owned()).unwrap();

        let v = d.get(&"a".to_owned());

        assert!(match v {
            Err(Error::CannotOpenFile) => true,
            _ => false,
        });
    }

    #[test]
    fn get_keys() {
        let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/test_db_4").unwrap();

        d.insert("a".to_owned(), 1).unwrap();
        d.insert("b".to_owned(), 2).unwrap();
        d.insert("c".to_owned(), 3).unwrap();

        let mut keys = d.get_keys().unwrap();

        assert_eq!(
            keys.sort(),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()].sort()
        )
    }

    #[test]
    fn complex_get() {
        let d: DiskMap<ComplexKey, HashSet<String>> = DiskMap::open_new("/tmp/test_db_5").unwrap();

        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
        pub struct ComplexKey {
            x: i32,
            y: i32,
            z: i32,
        }

        impl Display for ComplexKey {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}_{}_{}", self.x, self.y, self.z)
            }
        }

        impl From<String> for ComplexKey {
            fn from(s: String) -> Self {
                let s_ww = s.to_owned().replace(" ", "");

                let coords: Vec<&str> = s_ww.split('_').collect();

                let x = coords[0].parse::<i32>().unwrap();
                let y = coords[1].parse::<i32>().unwrap();
                let z = coords[2].parse::<i32>().unwrap();

                Self { x, y, z }
            }
        }

        let data = {
            let mut s = HashSet::new();
            s.insert("TEST".to_owned());

            s
        };

        let key = ComplexKey { x: 1, y: 1, z: 3 };

        d.insert(key.clone(), data.clone()).unwrap();

        let o = d.get(&key).unwrap();

        assert_eq!(o, data);
    }

    #[test]
    fn multithread() {
        let d: DiskMap<String, i32> = DiskMap::open_new("/tmp/test_db_6").unwrap();

        d.insert("a".to_owned(), 1).unwrap();
        d.insert("b".to_owned(), 2).unwrap();
        d.insert("c".to_owned(), 3).unwrap();

        let d1 = d.clone();
        thread::spawn(move || loop {
            let keys = d1.get_keys().unwrap();

            keys.iter().for_each(|e_ref| {
                d1.alter(e_ref, |_| 11).unwrap();
            });
        });

        let d2 = d.clone();
        thread::spawn(move || loop {
            let keys = d2.get_keys().unwrap();

            keys.iter().for_each(|e_ref| {
                d2.alter(e_ref, |_| 11).unwrap();
            });
        });

        thread::sleep(Duration::from_millis(2000));
    }
}
