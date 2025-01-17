use clipper::network::Package;
use log::{debug, info};
use std::io::{Read, Write};
use std::net::SocketAddr;
use std::net::TcpListener;
use std::net::TcpStream;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::RwLock;
use std::sync::Weak;
use std::thread;
use std::thread::JoinHandle;

use clipper::network::*;

mod parser;

struct Client {
    stream: TcpStream,
    package: Package,
    shared_key: Arc<Option<SharedKey>>,
}

struct ClientHandler {
    client: Arc<RwLock<Client>>,
    listen_stream: TcpStream,
    server_package: Arc<RwLock<Package>>,
}

impl ClientHandler {
    fn new(client: Arc<RwLock<Client>>, server_package: Arc<RwLock<Package>>) -> Self {
        let tcp_clone = client
            .read()
            .unwrap()
            .stream
            .try_clone()
            .expect("Unable to clone TcpStream");
        Self {
            client,
            listen_stream: tcp_clone,
            server_package,
        }
    }

    fn start_listener(self, broadcaster: Arc<Broadcaster>) -> JoinHandle<()> {
        thread::spawn(move || {
            let client = self.client;
            let mut stream = self.listen_stream;
            let server_package = self.server_package;
            let shared_key = client.read().unwrap().shared_key.clone();
            loop {
                let package_received = receive_package(&mut stream, &shared_key);
                if let Ok(package) = package_received {
                    if !matches!(package, Package::Empty) {
                        if client.read().unwrap().package != package {
                            client.write().unwrap().package = package;
                        }
                        if *server_package.read().unwrap() < client.read().unwrap().package {
                            *server_package.write().unwrap() =
                                client.read().unwrap().package.clone();
                            broadcaster.boardcast();
                        }
                    }
                } else if let Err(_) = package_received {
                    // server disconnected
                    break;
                }
            }
        })
    }
}

// This is needed to make clients and package thread-safe without putting the entire server under Arc and Rwlock
struct Broadcaster {
    clients: Weak<RwLock<Vec<Arc<RwLock<Client>>>>>,
    package: Weak<RwLock<Package>>,
}

impl Broadcaster {
    fn boardcast(&self) {
        let _arc_package = self.package.upgrade().expect("Server disconnected");
        let clients = self.clients.upgrade().expect("Server disconnected");

        let package = _arc_package.read().unwrap();
        let mut package_to_remove = vec![];

        info!("Broadcasting: {}", package);

        for (ind, client) in clients.read().unwrap().iter().enumerate() {
            if client.read().unwrap().package != *package {
                let mut target = client.write().unwrap();
                let key = target.shared_key.clone();
                if send_package(package.deref(), &mut target.stream, &key).is_err() {
                    debug!("Client disconnected");
                    package_to_remove.push(ind);
                }
            }
        }
        for i in package_to_remove {
            clients.write().unwrap().swap_remove(i);
        }
        debug!("Broadcasting done");
    }
}

struct Server {
    addr: SocketAddr,
    clients: Arc<RwLock<Vec<Arc<RwLock<Client>>>>>,
    package: Arc<RwLock<Package>>,
    broadcaster: Arc<Broadcaster>,
}

impl Server {
    fn new(addr: SocketAddr) -> Self {
        let clients = Arc::new(RwLock::new(Vec::new()));
        let package = Arc::new(RwLock::new(Package::default()));
        let broadcaster = Broadcaster {
            clients: Arc::downgrade(&clients),
            package: Arc::downgrade(&package),
        };
        Self {
            addr,
            clients,
            package,
            broadcaster: Arc::new(broadcaster),
        }
    }

    fn start(&mut self) -> std::io::Result<()> {
        let listener = TcpListener::bind(self.addr)?;
        debug!("Server started: {}", listener.local_addr().unwrap());

        for stream in listener.incoming() {
            match stream {
                Ok(mut stream) => {
                    debug!("New connection: {}", stream.peer_addr().unwrap());
                    let session = receive_session(&mut stream).expect("Failed to receive session");
                    let mut shared_key = Arc::new(None);

                    if session.use_encryption {
                        use rand_core::OsRng;
                        use x25519_dalek::{EphemeralSecret, PublicKey};
                        let server_private = EphemeralSecret::random_from_rng(OsRng);
                        let server_public = PublicKey::from(&server_private);

                        stream.write_all(server_public.as_bytes())?;

                        let mut client_public_key = [0u8; 32];
                        stream.read_exact(&mut client_public_key)?;

                        let client_public = PublicKey::from(client_public_key);
                        let shared_secret = server_private.diffie_hellman(&client_public);

                        shared_key = Arc::new(Some(SharedKey::from(shared_secret.as_bytes())));

                        debug!("Shared key: {:?}", shared_key);
                    }

                    let client = Client {
                        stream,
                        package: Package::default(),
                        shared_key,
                    };

                    let shared_client = Arc::new(RwLock::new(client));
                    self.add_client(Arc::clone(&shared_client));

                    let client_handler =
                        ClientHandler::new(Arc::clone(&shared_client), Arc::clone(&self.package));
                    client_handler.start_listener(Arc::clone(&self.broadcaster));
                }
                Err(e) => {
                    eprintln!("Connection failed: {}", e);
                }
            }
        }

        Ok(())
    }

    fn add_client(&mut self, client: Arc<RwLock<Client>>) {
        self.clients.write().unwrap().push(Arc::clone(&client));
    }
}

fn main() {
    env_logger::init();

    use crate::parser::*;

    let args = Args::parse();
    let socket = args.socket;
    let mut server = Server::new(socket);
    let _ = server.start().expect("Unable to bind to socket {socket}");
}
