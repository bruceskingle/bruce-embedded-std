//! ESP-IDF's NVS, the key-value store a device keeps its settings, keys and logs in, for the simulator: a partition is a
//! folder, see EspNvsPartition::at, each namespace a JSON file in it, written whole whenever a value changes, with the
//! same methods as esp_idf_svc::nvs::EspNvs, so that code which uses them compiles for the computer as it is.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::SimError;

/// The kind of partition, which ESP-IDF has several of, and the simulator one, NvsDefault.
#[derive(Clone, Copy, Debug)]
pub struct NvsDefault;

/// A partition: the folder its namespaces are kept in.
#[derive(Debug)]
pub struct EspNvsPartition<T> {
    folder: Arc<PathBuf>,
    _kind: PhantomData<T>,
}

impl<T> Clone for EspNvsPartition<T> {
    fn clone(&self) -> Self {
        EspNvsPartition {
            folder: self.folder.clone(),
            _kind: PhantomData,
        }
    }
}

/// The partition ESP-IDF's code takes, see EspNvsPartition::at.
pub type EspDefaultNvsPartition = EspNvsPartition<NvsDefault>;

impl<T> EspNvsPartition<T> {
    /// The partition kept in folder, which is made if it is not there.
    pub fn at(folder: impl Into<PathBuf>) -> Result<Self, SimError> {
        let folder = folder.into();
        std::fs::create_dir_all(&folder)?;
        Ok(EspNvsPartition {
            folder: Arc::new(folder),
            _kind: PhantomData,
        })
    }

    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

/// The values of each namespace file which is open, shared by every EspNvs of it, as ESP-IDF's handles share the
/// flash, so that one sees what another wrote.
static OPEN: Mutex<BTreeMap<PathBuf, Arc<Mutex<BTreeMap<String, Value>>>>> = Mutex::new(BTreeMap::new());

/// A namespace of a partition, with ESP-IDF's methods. Each value is kept with its type, as NVS does, e.g.
/// {"u8": 1}, {"str": "text"} or {"blob": [1, 2]}, and is read only as that type.
pub struct EspNvs<T> {
    file: PathBuf,
    values: Arc<Mutex<BTreeMap<String, Value>>>,
    _kind: PhantomData<T>,
}

impl<T> EspNvs<T> {
    pub fn new(partition: EspNvsPartition<T>, namespace: &str, _read_write: bool) -> Result<Self, SimError> {
        if namespace.is_empty() || namespace.len() > 15 || namespace.contains(['/', '\\', '.']) {
            return Err(SimError::new(format!("{namespace:?} is not a namespace NVS allows")));
        }
        let file = partition.folder.join(format!("{namespace}.json"));
        let values = OPEN
            .lock()
            .unwrap()
            .entry(file.clone())
            .or_insert_with(|| {
                let values = std::fs::read(&file)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                    .unwrap_or_default();
                Arc::new(Mutex::new(values))
            })
            .clone();
        Ok(EspNvs {
            file,
            values,
            _kind: PhantomData,
        })
    }

    /// The value of key, as kind, if it is that kind.
    fn get(&self, key: &str, kind: &str) -> Option<Value> {
        self.values
            .lock()
            .unwrap()
            .get(key)
            .and_then(|value| value.get(kind))
            .cloned()
    }

    /// Sets key to value, as kind, and saves the namespace, by writing a new file and renaming it over the old, so
    /// that a crash leaves the old or the new, never part of one.
    fn set(&self, key: &str, kind: &str, value: Value) -> Result<(), SimError> {
        if key.is_empty() || key.len() > 15 {
            return Err(SimError::new(format!("{key:?} is not a key NVS allows")));
        }
        let mut values = self.values.lock().unwrap();
        values.insert(key.to_string(), json!({ kind: value }));
        self.save(&values)
    }

    fn save(&self, values: &BTreeMap<String, Value>) -> Result<(), SimError> {
        let temporary = self.file.with_extension("json.new");
        std::fs::write(
            &temporary,
            serde_json::to_vec_pretty(values).map_err(|error| SimError::new(format!("{error}")))?,
        )?;
        std::fs::rename(&temporary, &self.file)?;
        Ok(())
    }

    /// The keys, with their types, of every value, or of those of data_type, as ESP-IDF's iterator gives them.
    pub fn keys(&self, data_type: Option<NvsDataType>) -> Result<EspNvsKeys, SimError> {
        let keys = self
            .values
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(key, value)| {
                let kind = value.as_object()?.keys().next()?.clone();
                let kind = NvsDataType::named(&kind)?;
                (data_type.is_none() || data_type == Some(kind)).then(|| (key.clone(), kind))
            })
            .collect::<Vec<_>>();
        Ok(EspNvsKeys(keys.into_iter()))
    }

    pub fn contains(&self, key: &str) -> Result<bool, SimError> {
        Ok(self.values.lock().unwrap().contains_key(key))
    }

    pub fn remove(&self, key: &str) -> Result<bool, SimError> {
        let mut values = self.values.lock().unwrap();
        let removed = values.remove(key).is_some();
        if removed {
            self.save(&values)?;
        }
        Ok(removed)
    }

    pub fn erase_all(&self) -> Result<(), SimError> {
        let mut values = self.values.lock().unwrap();
        values.clear();
        self.save(&values)
    }

    pub fn str_len(&self, key: &str) -> Result<Option<usize>, SimError> {
        // With its nul, as ESP-IDF counts it.
        Ok(self
            .get(key, "str")
            .and_then(|value| value.as_str().map(|text| text.len() + 1)))
    }

    /// The string, in buf, if it fits, as ESP-IDF's, which fails if it does not.
    pub fn get_str<'a>(&self, key: &str, buf: &'a mut [u8]) -> Result<Option<&'a str>, SimError> {
        let Some(text) = self.get(key, "str").and_then(|value| value.as_str().map(String::from)) else {
            return Ok(None);
        };
        if text.len() >= buf.len() {
            return Err(SimError::new(format!("The value of {key} does not fit in its buffer")));
        }
        buf[..text.len()].copy_from_slice(text.as_bytes());
        buf[text.len()] = 0;
        Ok(Some(std::str::from_utf8(&buf[..text.len()]).unwrap_or_default()))
    }

    pub fn set_str(&self, key: &str, value: &str) -> Result<(), SimError> {
        self.set(key, "str", json!(value))
    }

    pub fn blob_len(&self, key: &str) -> Result<Option<usize>, SimError> {
        Ok(self.get(key, "blob").and_then(|value| value.as_array().map(Vec::len)))
    }

    pub fn get_blob<'a>(&self, key: &str, buf: &'a mut [u8]) -> Result<Option<&'a [u8]>, SimError> {
        let Some(bytes) = self.get(key, "blob").and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .map(|item| item.as_u64().unwrap_or_default() as u8)
                    .collect::<Vec<u8>>()
            })
        }) else {
            return Ok(None);
        };
        if bytes.len() > buf.len() {
            return Err(SimError::new(format!("The value of {key} does not fit in its buffer")));
        }
        buf[..bytes.len()].copy_from_slice(&bytes);
        Ok(Some(&buf[..bytes.len()]))
    }

    pub fn set_blob(&self, key: &str, value: &[u8]) -> Result<(), SimError> {
        self.set(key, "blob", json!(value))
    }
}

/// The type of a value, as ESP-IDF's NvsDataType.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NvsDataType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    Str,
    Blob,
}

impl NvsDataType {
    /// The type of the kind a value is kept as in the file, see EspNvs.
    fn named(kind: &str) -> Option<Self> {
        Some(match kind {
            "u8" => NvsDataType::U8,
            "i8" => NvsDataType::I8,
            "u16" => NvsDataType::U16,
            "i16" => NvsDataType::I16,
            "u32" => NvsDataType::U32,
            "i32" => NvsDataType::I32,
            "u64" => NvsDataType::U64,
            "i64" => NvsDataType::I64,
            "str" => NvsDataType::Str,
            "blob" => NvsDataType::Blob,
            _ => return None,
        })
    }
}

/// The keys of a namespace, see EspNvs::keys.
pub struct EspNvsKeys(std::vec::IntoIter<(String, NvsDataType)>);

impl EspNvsKeys {
    pub fn next_key(&mut self) -> Option<(String, NvsDataType)> {
        self.0.next()
    }
}

/// Getters and setters of the integer types, all alike.
macro_rules! integers {
    ($($get:ident, $set:ident, $type:ty, $kind:literal;)*) => {
        impl<T> EspNvs<T> {
            $(
                pub fn $get(&self, key: &str) -> Result<Option<$type>, SimError> {
                    Ok(self.get(key, $kind).and_then(|value| serde_json::from_value(value).ok()))
                }

                pub fn $set(&self, key: &str, value: $type) -> Result<(), SimError> {
                    self.set(key, $kind, json!(value))
                }
            )*
        }
    };
}

integers! {
    get_u8, set_u8, u8, "u8";
    get_i8, set_i8, i8, "i8";
    get_u16, set_u16, u16, "u16";
    get_i16, set_i16, i16, "i16";
    get_u32, set_u32, u32, "u32";
    get_i32, set_i32, i32, "i32";
    get_u64, set_u64, u64, "u64";
    get_i64, set_i64, i64, "i64";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_kept_by_type_and_survive_reopening() {
        let folder = std::env::temp_dir().join(format!("bruce-sim-nvs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let partition = EspNvsPartition::<NvsDefault>::at(&folder).unwrap();
        let nvs = EspNvs::new(partition.clone(), "device", true).unwrap();
        nvs.set_str("name", "rlcd").unwrap();
        nvs.set_u8("adopted", 1).unwrap();
        nvs.set_blob("log", &[1, 2, 3]).unwrap();
        assert_eq!(nvs.get_u32("adopted").unwrap(), None);

        // As a fresh start sees it, from the file.
        OPEN.lock().unwrap().clear();
        let nvs = EspNvs::new(partition, "device", false).unwrap();
        let mut buffer = [0; 16];
        assert_eq!(nvs.get_str("name", &mut buffer).unwrap(), Some("rlcd"));
        assert_eq!(nvs.get_u8("adopted").unwrap(), Some(1));
        assert_eq!(nvs.blob_len("log").unwrap(), Some(3));
        assert_eq!(nvs.get_blob("log", &mut buffer).unwrap(), Some(&[1, 2, 3][..]));
        assert!(nvs.get_str("name", &mut [0; 4]).is_err());
        assert!(nvs.remove("name").unwrap());
        assert_eq!(nvs.get_str("name", &mut buffer).unwrap(), None);
        let _ = std::fs::remove_dir_all(&folder);
    }
}
