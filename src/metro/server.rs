use std::collections::HashMap;
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use uuid::Uuid;
use serde_json::to_string;

use crate::metro::handle_pkg;
use crate::metro::protocol::{InformGameEnd, Packages};
use crate::package::storage::Storage;

// ─── Routing enum ─────────────────────────────────────────────────────────────

/// Returned by [`handle`] to decide where the response is delivered.
#[derive(Debug)]
pub enum Routing {
    /// Send the response only back to the originating sender.
    Return(String),
    ReturnConAdmin((String, Uuid, String)),
    /// Send the response to every UUID listed in `targets`.
    Broadcast {
        message: String,
        targets: Vec<Uuid>,
    },
}
// ─── Per-user handle ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct UserHandle {
    id: Uuid,
    tx: mpsc::UnboundedSender<String>,
}

impl UserHandle {
    /// Enqueue an outbound text message. Returns `false` if the user is gone.
    fn send(&self, msg: String) -> bool {
        self.tx.send(msg).is_ok()
    }
}

// ─── Shared state ─────────────────────────────────────────────────────────────

type Users = Arc<RwLock<HashMap<Uuid, UserHandle>>>;

// ─── Server ───────────────────────────────────────────────────────────────────

/// WebSocket server.  Clone freely – all clones share the same user map.
#[derive(Clone)]
pub struct Server {
    users: Users,
    game: Storage
}

impl Server {
    pub fn new() -> Self {
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            game: Storage::new()
        }
    }

    /// Bind to `addr` and accept WebSocket connections forever.
    ///
    /// Each accepted connection gets a fresh UUID, is stored in the map,
    /// and is driven in its own Tokio task.
    pub async fn listen(self, addr: &str) -> anyhow::Result<()> {
        let listener = TcpListener::bind(addr).await?;
        println!("Listening on ws://{addr}");

        loop {
            let (stream, peer) = listener.accept().await?;
            println!("TCP connection from {peer}");
            let server = self.clone();
            tokio::spawn(async move {
                if let Err(e) = server.handle_connection(stream).await {
                    eprintln!("Connection error: {e}");
                }
            });
        }
    }

    // ── Internal: one task per connection ────────────────────────────────────

    async fn handle_connection(&self, stream: TcpStream) -> anyhow::Result<()> {
        // Upgrade TCP → WebSocket
        let ws = accept_async(stream).await?;
        let (mut ws_tx, mut ws_rx) = ws.split();

        // Assign a UUID and create the outbound channel
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        self.users.write().await.insert(id, UserHandle { id, tx });
        println!("User connected: {id}");

        // Task: forward messages from the channel → WebSocket sink
        let write_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_tx.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
            // Cleanly close the sink when the channel drains
            let _ = ws_tx.close().await;
        });

        // Drive the WebSocket read loop in the current task
        while let Some(frame) = ws_rx.next().await {
            match frame {
                Ok(Message::Text(text)) => {
                    // `text` is a String – bytes never reach `handle()`
                    self.route(id, text.to_string()).await;
                }
                Ok(Message::Close(_)) | Err(_) => break,
                // Ignore binary, ping, pong frames
                _ => {}
            }
        }

        // Clean up
        self.users.write().await.remove(&id);
        write_task.abort();
        println!("User disconnected: {id}");
        Ok(())
    }

    // ── Routing ───────────────────────────────────────────────────────────────

    /// Call `handle()` and deliver the result to the right user(s).
    async fn route(&self, sender_id: Uuid, input: String) {
        let routing = handle(input, &self.game, sender_id);
        let users = self.users.read().await;

        match routing {
            Routing::Return(msg) => {
                if let Some(user) = users.get(&sender_id) {
                    user.send(msg);
                }
            }
            Routing::Broadcast { message, targets } => {
                for target_id in &targets {
                    if let Some(user) = users.get(target_id) {
                        user.send(message.clone());
                    }
                }
            }
            Routing::ReturnConAdmin((msg, admin, msg_admin)) => {
                if let Some(user) = users.get(&sender_id) {
                    user.send(msg);
                }
                if let Some(user) = users.get(&admin) {
                    user.send(msg_admin);
                }
            },
        }
    }

    /// Snapshot of every currently-connected UUID.
    /// Pass this into `handle()` (or use it at the call site) when you need
    /// to build an "everyone" broadcast list.
    pub async fn connected_ids(&self) -> Vec<Uuid> {
        self.users.read().await.keys().copied().collect()
    }
}

// ─── Application logic ────────────────────────────────────────────────────────

/// Decide what to do with an inbound WebSocket text message.
///
/// **Always receives a `String`** – binary frames are dropped before this
/// function is ever called.
///
/// Replace the body with your real application logic.
/// If you need the live user list to populate `targets`, call
/// `Server::connected_ids().await` before invoking `route()` and thread the
/// IDs through here, or promote this to an `async` method on `Server`.
/// 
/// 
/// 
pub enum IsMultReturn {
    Data(InformGameEnd),
    Nothing,
}
fn is_mult(p: &Packages) -> IsMultReturn {
    match p {
        Packages::InformGameEnd(a) => {
            return IsMultReturn::Data(a.clone());
        }
        _ => {
            return IsMultReturn::Nothing;
        }
    }


}

pub fn handle(input: String, game: &Storage, uuid: Uuid) -> Routing {
    if let Some(body) = input.strip_prefix("/broadcast ") {
        //Parse out the Enum
        let p: Packages = match serde_json::from_str(&input) {
            Ok(a) => {
                a
            },
            Err(a) => {
                return Routing::Return("Invalid!".to_string());
            }
        };

        //check if mult
        let mult_res = is_mult(&p);
        match mult_res {
            IsMultReturn::Data(inform_game_end) => {
                let h: String = serde_json::to_string(&Packages::InformGameSend()).unwrap();
                return Routing::Broadcast {
                    message: h,
                    targets: inform_game_end.targets
                };
            },
            IsMultReturn::Nothing => {},
        }

        //all weird stuff is delt with!
        let res = match handle_pkg::handle_pkg(game, p, uuid) {
            Ok(a) => {
                let res_str = match serde_json::to_string(&a.0) {
                    Ok(e) => {
                        e
                    },
                    Err(e) => {
                        return Routing::Return("Invalid!".to_string());
                    }
                };

                if let Some(admin_stuff) = a.1 {
                    let admin_str = match serde_json::to_string(&admin_stuff.0) {
                        Ok(e) => {
                            e
                        },
                        Err(e) => {
                            return Routing::Return("Invalid!".to_string());
                        }
                    };

                    return Routing::ReturnConAdmin((res_str, admin_stuff.1, admin_str));

                }

                return Routing::Return(res_str);
            },
            Err(a) => {
                return Routing::Return("Invalid!".to_string());
            },
        };
    } else {
        // Default: echo back to the sender.
        Routing::Return(format!("echo: {input}"))
    }
}