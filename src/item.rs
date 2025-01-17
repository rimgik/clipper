use core::fmt;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::ffi::OsString;
use std::path::Path;
use std::path::*;

const MAX_SYMLINK_RECURSION_DEPTH: usize = 100;

#[cfg(target_os = "macos")]
mod mac_item {
    use super::*;
    use crate::mac;
    use objc2::rc::Retained;
    use objc2_foundation::*;

    #[derive(Debug, PartialEq, Eq, Default, Hash, Clone)]
    pub struct RetainedDataWrapper(Retained<NSData>);
    struct RetainedDataWrapperVisitor;

    impl Serialize for RetainedDataWrapper {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serializer.serialize_newtype_struct("RetainedDataWrapper", self.0.bytes())
        }
    }

    impl<'de> Deserialize<'de> for RetainedDataWrapper {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_bytes(RetainedDataWrapperVisitor)
        }
    }

    impl<'de> Visitor<'de> for RetainedDataWrapperVisitor {
        type Value = RetainedDataWrapper;
        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("bytes data")
        }
        fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(RetainedDataWrapper(NSData::with_bytes(v)))
        }
    }

    impl From<Retained<NSData>> for RetainedDataWrapper {
        fn from(value: Retained<NSData>) -> Self {
            Self(value)
        }
    }

    impl RetainedDataWrapper {
        pub fn len(&self) -> usize {
            self.0.len()
        }
    }

    impl AsRef<[u8]> for RetainedDataWrapper {
        fn as_ref(&self) -> &[u8] {
            self.0.bytes()
        }
    }

    impl TryFrom<mac::Item> for TransferableItem {
        type Error = mac::Error;
        fn try_from(value: mac::Item) -> Result<Self, Self::Error> {
            match value {
                mac::Item::File(data, ext) => Ok(Self::File {
                    file_name: ext,
                    data: data.into(),
                }),
                mac::Item::Text(text) => Ok(Self::Text {
                    text: text.to_string(),
                }),
                mac::Item::FileUrl(url) => {
                    let path: PathBuf = url.to_string().into();

                    let mut depth = 0;
                    while path.is_symlink() && depth < MAX_SYMLINK_RECURSION_DEPTH {
                        path.read_link().expect("Error reading symlink at: {path}");
                        depth += 1;
                    }

                    if depth >= MAX_SYMLINK_RECURSION_DEPTH {
                        panic!("Maximum depth reached while reading symlink")
                    }

                    if path.is_dir() {
                        unimplemented!();
                    } else if path.is_file() {
                        let file_name = path.file_name().unwrap().to_os_string();
                        let file_data = std::fs::read(path).unwrap();
                        Ok(Self::File {
                            file_name,
                            data: NSData::from_vec(file_data).into(),
                        })
                    } else {
                        Err(Self::Error::UnsupportedType)
                    }
                }
                mac::Item::Unsupported() => Err(Self::Error::UnsupportedType),
            }
        }
    }
}

#[cfg(target_os = "macos")]
type Data = mac_item::RetainedDataWrapper;
#[cfg(not(target_os = "macos"))]
type Data = Vec<u8>;

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone)]
pub enum TransferableItem {
    File {
        file_name: OsString,
        data: Data,
    },
    Text {
        text: String,
    },
    // Put this struct at last, because of this bug: https://github.com/bincode-org/bincode/issues/184
    #[serde(skip)]
    Folder {
        // dir: ReadDir,
    },
}

impl TransferableItem {
    pub fn write_to_dir<P: AsRef<Path>>(&self, dir: P) -> () {
        match self {
            Self::File { file_name, data } => {
                std::fs::write(dir.as_ref().join(file_name), data).unwrap()
            }
            Self::Folder { .. } => unimplemented!(),
            Self::Text { text } => std::fs::write(dir.as_ref().join("out.txt"), text).unwrap(),
        }
    }
}

impl From<String> for TransferableItem {
    fn from(value: String) -> Self {
        TransferableItem::Text { text: value }
    }
}

impl fmt::Display for TransferableItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::File { file_name, data } => {
                write!(f, "File name: {:?}; File size: {}", file_name, data.len())
            }
            Self::Text { text } => write!(f, "Text: {text}"),
            Self::Folder { .. } => write!(f, "DIR"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    mod mac_test {
        use super::super::mac_item::RetainedDataWrapper;
        use super::*;
        use crate::mac;
        use objc2_foundation::*;

        #[test]
        fn data_wrapper_mac_serialize_json_test() {
            let text = b"Hello World!";
            let data_wrapper = RetainedDataWrapper::from(NSData::with_bytes(text));
            let json = serde_json::to_string(&data_wrapper).expect("Failed to serialize");
            println!("{:?}", text); // Prints: [72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100]
            println!("{}", json);
        }

        #[test]
        fn data_wrapper_mac_serialize_bincode_test() {
            let text = b"Hello World!";
            let data_wrapper = RetainedDataWrapper::from(NSData::with_bytes(text));
            let serialized = bincode::serialize(&data_wrapper).expect("Failed to serialize");
            let deserialized: RetainedDataWrapper =
                bincode::deserialize(&serialized).expect("Failed to deserialize");
            println!("{serialized:?}");
            println!("{deserialized:?}");
            assert_eq!(data_wrapper, deserialized);
        }

        #[test]
        fn transferableitem_serialize_bincode_test() {
            mac::write_text("Hello".to_string());
            let pasteboard_item = mac::read().unwrap();
            let item = TransferableItem::try_from(pasteboard_item).unwrap();
            let serialized = bincode::serialize(&item).unwrap();
            let deserialized: TransferableItem = bincode::deserialize(&serialized).unwrap();
            assert_eq!(deserialized, item);
        }
    }
}
