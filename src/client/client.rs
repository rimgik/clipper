use clipper::network::Package;
use log::{debug, info};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use clipper::network::*;

mod parser;

const POOLING_TIME: Duration = Duration::from_millis(200);

struct Server {
    stream: TcpStream,
    listen_stream: TcpStream,
    shared_key: Arc<Option<SharedKey>>,
}

#[allow(unused)]
fn calculate_hash<T: Hash>(t: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    t.hash(&mut hasher);
    hasher.finish()
}

impl Server {
    fn connect(addr: SocketAddr) -> Self {
        info!("Connecting to {addr}");
        let stream = TcpStream::connect(addr).expect("Unable to connect to server");
        info!("Connected to {addr}");
        let stream_clone = stream.try_clone().expect("Unable to clone TcpStream");
        Self {
            stream,
            listen_stream: stream_clone,
            shared_key: Arc::new(None),
        }
    }

    fn start(&mut self, session: SessionInfo) {
        use std::thread;

        // handshake
        send_session(&mut self.stream, &session).unwrap();

        if session.use_encryption {
            use rand_core::OsRng;
            use std::io::Read;
            use std::io::Write;
            use x25519_dalek::{EphemeralSecret, PublicKey};
            let client_private = EphemeralSecret::random_from_rng(OsRng);
            let client_public = PublicKey::from(&client_private);

            let mut server_public_key = [0u8; 32];
            self.stream.read_exact(&mut server_public_key).unwrap();

            self.stream.write_all(client_public.as_bytes()).unwrap();

            let server_public = PublicKey::from(server_public_key);
            let shared_secret = client_private.diffie_hellman(&server_public);
            self.shared_key = Arc::new(Some(SharedKey::from(shared_secret.as_bytes())));

            debug!("Shared key: {:?}", self.shared_key);
        }

        thread::scope(|s| {
            s.spawn(|| Server::start_sender(&mut self.stream, &self.shared_key));
            s.spawn(|| Server::start_listener(&mut self.listen_stream, &self.shared_key));
        });
    }

    #[cfg(target_os = "macos")]
    fn start_sender(stream: &mut TcpStream, shared_key: &Option<SharedKey>) {
        let mut current_count = mac::get_count();
        loop {
            // This is ugly but appkit doesn't provide proper API for monitoring clipboard change
            let t = mac::get_count();
            if current_count < t {
                send_package(&generate_package(), stream, shared_key).unwrap();
            }
            current_count = t;
            std::thread::sleep(POOLING_TIME);
        }
    }

    #[cfg(target_os = "macos")]
    fn start_listener(stream: &mut TcpStream, shared_key: &Option<SharedKey>) {
        use clipper::item::TransferableItem;
        loop {
            match receive_package(stream, shared_key) {
                Ok(package) => {
                    if let Package::Item { item, .. } = package {
                        println!("writing text");
                        match item {
                            TransferableItem::File { .. } => unimplemented!(),
                            TransferableItem::Folder { .. } => unimplemented!(),
                            TransferableItem::Text { text } => mac::write_text(text),
                        }
                    }
                }
                Err(err) => panic!("Unable to connect to server: {err}"),
            }
        }
    }

    #[cfg(target_os = "windows")]
    fn start_sender(stream: &mut TcpStream, shared_key: &Option<SharedKey>) {
        let mut current_item = get_current_item();
        loop {
            let t = get_current_item();
            if calculate_hash(&t) != calculate_hash(&current_item) {
                send_package(&generate_package(), stream, shared_key).unwrap();
            }
            current_item = t;
            std::thread::sleep(POOLING_TIME);
        }
    }

    #[cfg(target_os = "windows")]
    fn start_listener(stream: &mut TcpStream, shared_key: &Option<SharedKey>) {
        use clipper::item::TransferableItem;
        use clipper::windows;
        loop {
            match receive_package(stream, shared_key) {
                Ok(package) => {
                    if let Package::Item { item, .. } = package {
                        match item {
                            TransferableItem::File { .. } => unimplemented!(),
                            TransferableItem::Folder { .. } => unimplemented!(),
                            TransferableItem::Text { text } => windows::write_text(text),
                        }
                    }
                }
                Err(err) => panic!("Unable to connect to server: {err}"),
            }
        }
    }
}

#[cfg(target_os = "macos")]
use clipper::mac;

#[cfg(target_os = "windows")]
fn get_current_item() -> Result<clipper::item::TransferableItem, clipper::windows::Error> {
    use clipper::{item::TransferableItem, windows};

    let text = windows::read_text();
    match text {
        Ok(t) => Ok(TransferableItem::from(t)),
        Err(err) => Err(err),
    }
}

#[cfg(target_os = "windows")]
fn generate_package() -> Package {
    use clipper::item::TransferableItem;

    match get_current_item() {
        Ok(item) => match &item {
            TransferableItem::Text { .. } => Package::from(item),
            _ => Package::Empty,
        },
        Err(err) => panic!("Unsupported type"),
    }
}

#[cfg(target_os = "macos")]
fn get_current_item() -> Result<clipper::item::TransferableItem, mac::Error> {
    use clipper::item::TransferableItem;

    TransferableItem::try_from(mac::read().unwrap())
}

#[cfg(target_os = "macos")]
fn generate_package() -> Package {
    use clipper::item::TransferableItem;

    match get_current_item() {
        Ok(item) => match &item {
            TransferableItem::Text { .. } => Package::from(item),
            _ => Package::Empty,
        },
        Err(err) => panic!("Unsupported type: {err:?}"),
    }
}

fn main() {
    use parser::*;
    let args = Args::parse();
    let addr = args.socket;

    let log_level = if args.verbose { "debug" } else { "info" };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(log_level)).init();

    let mut server = Server::connect(addr);

    let session = SessionInfo {
        os: std::env::consts::OS.to_string(),
        use_encryption: args.encrypted,
    };

    server.start(session);
}
