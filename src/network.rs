use log::debug;
use orion::aead;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Read;
use std::io::Write;
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::item::*;

#[cfg(target_os = "macos")]
use crate::mac;

#[derive(Debug)]
pub struct SharedKey {
    pub key: aead::SecretKey,
}

impl SharedKey {
    fn generate() -> Self {
        Self {
            key: aead::SecretKey::default(),
        }
    }
}

impl From<[u8; 32]> for SharedKey {
    fn from(value: [u8; 32]) -> Self {
        Self {
            key: aead::SecretKey::from_slice(&value).unwrap(),
        }
    }
}

impl From<&[u8; 32]> for SharedKey {
    fn from(value: &[u8; 32]) -> Self {
        Self {
            key: aead::SecretKey::from_slice(value).unwrap(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone)]
pub struct SessionInfo {
    pub os: String,
    pub use_encryption: bool,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone)]
pub enum Package {
    Empty,
    Item { time: u64, item: TransferableItem },
}

impl fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Package::Empty => write!(f, "Package::Empty"),
            Package::Item { time, item } => {
                write!(f, "Package::Item{{ Time: {}; {} }}", time, item)
            }
        }
    }
}

impl PartialOrd for Package {
    fn lt(&self, other: &Self) -> bool {
        match self {
            Self::Empty => match other {
                Self::Empty => false,
                Self::Item { .. } => true,
            },
            Self::Item { time, .. } => match other {
                Self::Empty => false,
                Self::Item { time: time2, .. } => time < time2,
            },
        }
    }
    fn le(&self, other: &Self) -> bool {
        self.lt(other) || self.eq(other)
    }

    fn gt(&self, other: &Self) -> bool {
        other.lt(self)
    }

    fn ge(&self, other: &Self) -> bool {
        self.gt(other) || self.eq(other)
    }

    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.lt(other) {
            Some(std::cmp::Ordering::Less)
        } else if self.gt(other) {
            Some(std::cmp::Ordering::Greater)
        } else if self.eq(other) {
            Some(std::cmp::Ordering::Equal)
        } else {
            None
        }
    }
}

impl Default for Package {
    fn default() -> Self {
        Self::Empty
    }
}

#[cfg(target_os = "macos")]
impl TryFrom<mac::Item> for Package {
    type Error = mac::Error;
    fn try_from(value: mac::Item) -> Result<Self, Self::Error> {
        let item = TransferableItem::try_from(value)?;
        match item {
            TransferableItem::Text { .. } => {
                let time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs();
                Ok(Self::Item { time, item })
            }
            _ => Err(Self::Error::UnsupportedType),
        }
    }
}

impl From<TransferableItem> for Package {
    fn from(value: TransferableItem) -> Self {
        Self::Item {
            time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            item: value,
        }
    }
}

pub fn send_package(
    package: &Package,
    stream: &mut TcpStream,
    shared_key: &Option<SharedKey>,
) -> std::io::Result<()> {
    let mut bin_stream = bincode::serialize(package).expect("Failed to serialize");

    if let Some(key) = shared_key {
        bin_stream = aead::seal(&key.key, &bin_stream).expect("Failed to encrypt message");
    }

    let len = bin_stream.len();
    let bin_len = len.to_be_bytes();
    debug!("Sending {} bytes of data to {}", len, stream.peer_addr()?);
    // debug!("Raw bytes sent: {:?}", bin_stream);

    stream.write_all(&bin_len)?;
    stream.write_all(&bin_stream)?;
    debug!(
        "Successfully send {} bytes of data to {:?}",
        len,
        stream.peer_addr()?
    );
    Ok(())
}

pub fn receive_package(
    stream: &mut TcpStream,
    shared_key: &Option<SharedKey>,
) -> std::io::Result<Package> {
    let mut len_buffer = [0u8; 8];
    let _res = stream.read_exact(&mut len_buffer)?;
    let package_len = u64::from_be_bytes(len_buffer);

    debug!(
        "Incoming package of size {} from {}",
        package_len,
        stream.peer_addr()?
    );

    let mut buffer = vec![0u8; package_len as usize];
    let _ = stream.read_exact(&mut buffer)?;
    // debug!("Raw bytes received: {:?}", buffer);

    let package: Package;
    if let Some(key) = shared_key {
        buffer = aead::open(&key.key, &buffer).expect("Failed to decrypt message");
    }
    package = bincode::deserialize(&buffer).expect("Failed to deserialize");
    debug!("Package received ({}): {}", package_len, package);
    Ok(package)
}

pub fn send_session(stream: &mut TcpStream, session: &SessionInfo) -> std::io::Result<()> {
    let bin_stream = bincode::serialize(session).expect("Unable to serialize session");
    let len = bin_stream.len();
    let bin_len = len.to_be_bytes();

    stream.write_all(&bin_len)?;
    stream.write_all(&bin_stream)?;
    Ok(())
}

pub fn receive_session(stream: &mut TcpStream) -> std::io::Result<SessionInfo> {
    let mut len_buffer = [0u8; 8];
    stream.read_exact(&mut len_buffer)?;
    let len = u64::from_be_bytes(len_buffer);

    let mut buffer = vec![0u8; len as usize];
    let _ = stream.read_exact(&mut buffer)?;

    let session: SessionInfo = bincode::deserialize(&buffer).expect("Failed to deserialize");

    debug!("Received session: {:?}", session);

    Ok(session)
}

#[cfg(test)]
mod tests {
    use orion::aead;

    use super::{Package, TransferableItem};

    #[test]
    fn shared_key_encryption_test() {
        let key = aead::SecretKey::default();
        let msg = "Hello world".to_string();

        // Sender
        let package = Package::from(TransferableItem::from(msg));
        println!("Package: {package:?}");

        let bin_stream = bincode::serialize(&package).unwrap();
        let encrypted_bin_stream = aead::seal(&key, &bin_stream).unwrap();
        let decrypted_bin_stream = aead::open(&key, &encrypted_bin_stream).unwrap();

        assert_eq!(bin_stream, decrypted_bin_stream);

        let decrypted_payload: Package = bincode::deserialize(&decrypted_bin_stream).unwrap();

        assert_eq!(decrypted_payload, package);
    }
}
