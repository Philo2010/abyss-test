use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio::time::Instant;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use uuid::Uuid;

use crate::metro::handle_pkg;
use crate::metro::protocol::{
    InformGameEnd, LoginUserUpdateAdmin, Packages, ResumeReturn, ReturnForAdmin, UpdateInformAdmin,
};
use crate::package::storage::{GameDataError, ResumeInfo, Storage};

/// How often the server pings each client. The browser answers pings
/// automatically, so a missing pong means the connection is dead.
const PING_INTERVAL: Duration = Duration::from_secs(10);

/// How long we tolerate total silence (no data, ping or pong) before we treat
/// the socket as dropped and clean it up.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Pause before retrying `accept()` after a transient OS error.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(200);

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
    /// Send `ret` to the sender and `broadcast` to every UUID in `targets`.
    ReturnAndBroadcast {
        ret: String,
        broadcast: String,
        targets: Vec<Uuid>,
    },
    /// Send `ret` to the sender, a message to the admin, and `broadcast` to
    /// every UUID in `targets`. Needed when a submission both notifies the
    /// admin and ends the game (the broadcast) in the same message.
    ReturnAndBroadcastConAdmin {
        ret: String,
        admin: (Uuid, String),
        broadcast: String,
        targets: Vec<Uuid>,
    },
}
// ─── Per-user handle ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
struct UserHandle {
    id: Uuid,
    tx: mpsc::UnboundedSender<Message>,
}

impl UserHandle {
    /// Enqueue an outbound text message. Returns `false` if the user is gone.
    fn send(&self, msg: String) -> bool {
        self.tx.send(Message::Text(msg.into())).is_ok()
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
            // A single failed `accept()` (fd exhaustion, transient ECONNABORTED,
            // ...) must not take the whole server down.
            let (stream, peer) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    eprintln!("Accept error: {e}");
                    tokio::time::sleep(ACCEPT_BACKOFF).await;
                    continue;
                }
            };
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
        // Disable Nagle so small state updates are pushed immediately.
        let _ = stream.set_nodelay(true);

        // Upgrade TCP → WebSocket
        let ws = accept_async(stream).await?;
        let (mut ws_tx, mut ws_rx) = ws.split();

        // Assign a UUID and create the outbound channel. The UUID may change
        // later if the client resumes a previous session.
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        self.users
            .write()
            .await
            .insert(id, UserHandle { id, tx: tx.clone() });
        println!("User connected: {id} (pre-resume)");

        // Task: forward messages from the channel → WebSocket sink
        let write_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
            // Cleanly close the sink when the channel drains
            let _ = ws_tx.close().await;
        });

        // The identity this connection currently routes as.
        let mut conn_id = id;

        // Heartbeat: ping on an interval, and bail if we hear nothing back.
        let mut ping = tokio::time::interval(PING_INTERVAL);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_seen = Instant::now();

        loop {
            tokio::select! {
                _ = ping.tick() => {
                    if tx.send(Message::Ping(Vec::new().into())).is_err() {
                        break;
                    }
                }
                _ = tokio::time::sleep_until(last_seen + IDLE_TIMEOUT) => {
                    eprintln!("User {conn_id} timed out (no heartbeat)");
                    break;
                }
                frame = ws_rx.next() => {
                    match frame {
                        Some(Ok(Message::Text(text))) => {
                            last_seen = Instant::now();
                            let text = text.to_string();

                            // A resume request swaps this socket in for the
                            // dead one without losing the client's identity.
                            if let Ok(Packages::Resume(resume)) =
                                serde_json::from_str::<Packages>(&text)
                            {
                                match self.resume_user(conn_id, resume.uuid, tx.clone()).await {
                                    Ok(info) => {
                                        conn_id = resume.uuid;
                                        self.send_resume(&tx, resume.uuid, info);
                                    }
                                    Err(_) => {
                                        let err = Packages::Error("resume failed".to_string());
                                        let _ = tx.send(Message::Text(
                                            serde_json::to_string(&err).unwrap().into(),
                                        ));
                                    }
                                }
                                continue;
                            }

                            self.route(conn_id, text).await;
                        }
                        // Any inbound frame (including the browser's automatic
                        // pong reply) counts as proof of life.
                        Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {
                            last_seen = Instant::now();
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                        // Ignore binary frames
                        Some(Ok(Message::Binary(_))) | Some(Ok(Message::Frame(_))) => {}
                    }
                }
            }
        }

        // Clean up. Only remove ourselves: if another connection has since
        // resumed this identity, that newer handle must stay in the map.
        {
            let mut users = self.users.write().await;
            let current_is_us = users
                .get(&conn_id)
                .is_some_and(|handle| handle.tx.same_channel(&tx));
            if current_is_us {
                users.remove(&conn_id);
            }
        }
        write_task.abort();
        println!("User disconnected: {conn_id}");
        Ok(())
    }

    // ── Resume ────────────────────────────────────────────────────────────────

    /// Re-attach `old_id` to the socket behind `tx`, replacing any stale handle.
    ///
    /// Returns cloneable info about the identity, or `Err` if the UUID is unknown.
    async fn resume_user(
        &self,
        temp_id: Uuid,
        old_id: Uuid,
        tx: mpsc::UnboundedSender<Message>,
    ) -> Result<ResumeInfo, GameDataError> {
        // Look the identity up first: if the game is gone we don't want to have
        // already torn down the temporary handle.
        let info = self.game.resume_info(old_id)?;

        let mut users = self.users.write().await;
        users.remove(&temp_id);
        // Replacing the old handle drops its sender, which makes the previous
        // socket's writer drain and close, so the stale tab is kicked.
        users.insert(old_id, UserHandle { id: old_id, tx });
        Ok(info)
    }

    /// Send a resume acknowledgement, followed by a full replay of the admin
    /// table when an admin reconnects.
    fn send_resume(&self, tx: &mpsc::UnboundedSender<Message>, uuid: Uuid, info: ResumeInfo) {
        let ret = ResumeReturn {
            uuid,
            code: info.code,
            station: info.station,
            is_admin: info.is_admin,
            started: info.started,
            ended: info.ended,
        };
        let _ = tx.send(Message::Text(
            serde_json::to_string(&Packages::ResumeReturn(ret)).unwrap().into(),
        ));

        if !info.is_admin {
            return;
        }

        let Ok(snapshot) = self.game.snapshot(info.code) else {
            return;
        };

        for user in snapshot.users {
            let pkg = Packages::LoginUserUpdateAdmin(LoginUserUpdateAdmin {
                uuid: user.uuid,
                username: user.username,
                station: user.station,
            });
            let _ = tx.send(Message::Text(serde_json::to_string(&pkg).unwrap().into()));

            for patch in user.live {
                let pkg = Packages::UpdateInformAdmin(UpdateInformAdmin {
                    patch,
                    uuid: user.uuid,
                });
                let _ = tx.send(Message::Text(serde_json::to_string(&pkg).unwrap().into()));
            }
        }

        for sub in snapshot.submitted {
            let pkg = Packages::ReturnForAdmin(ReturnForAdmin {
                data: sub.data,
                defence: sub.defence,
                comment: sub.comment,
                game_id: info.code,
                user_uuid: sub.user_uuid,
            });
            let _ = tx.send(Message::Text(serde_json::to_string(&pkg).unwrap().into()));
        }
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
            Routing::ReturnAndBroadcast { ret, broadcast, targets } => {
                if let Some(user) = users.get(&sender_id) {
                    user.send(ret);
                }
                for target_id in &targets {
                    if let Some(user) = users.get(target_id) {
                        user.send(broadcast.clone());
                    }
                }
            },
            Routing::ReturnAndBroadcastConAdmin { ret, admin, broadcast, targets } => {
                if let Some(user) = users.get(&sender_id) {
                    user.send(ret);
                }
                if let Some(user) = users.get(&admin.0) {
                    user.send(admin.1);
                }
                for target_id in &targets {
                    if let Some(user) = users.get(target_id) {
                        user.send(broadcast.clone());
                    }
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
        //Parse out the Enum
        let p: Packages = match serde_json::from_str(&input) {
            Ok(a) => {
                a
            },
            Err(a) => {
                return Routing::Return("Invalid!".to_string());
            }
        };

        println!("Got package {:?}", p);

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
        match handle_pkg::handle_pkg(game, p, uuid) {
            Ok((main_pkg, admin_pkg, broadcast)) => {
                let res_str = match serde_json::to_string(&main_pkg) {
                    Ok(e) => e,
                    Err(_) => return Routing::Return("Invalid!".to_string()),
                };

                // Serialise the admin message up front so a submission that
                // also ends the game can deliver both.
                let admin_stuff = match admin_pkg {
                    Some(admin_stuff) => {
                        let admin_str = match serde_json::to_string(&admin_stuff.0) {
                            Ok(e) => e,
                            Err(_) => return Routing::Return("Invalid!".to_string()),
                        };
                        Some((admin_stuff.1, admin_str))
                    }
                    None => None,
                };

                if let Some((broadcast_pkg, targets)) = broadcast {
                    let broadcast_str = match serde_json::to_string(&broadcast_pkg) {
                        Ok(e) => e,
                        Err(_) => return Routing::Return("Invalid!".to_string()),
                    };
                    if let Some((admin, admin_str)) = admin_stuff {
                        return Routing::ReturnAndBroadcastConAdmin {
                            ret: res_str,
                            admin: (admin, admin_str),
                            broadcast: broadcast_str,
                            targets,
                        };
                    }
                    return Routing::ReturnAndBroadcast {
                        ret: res_str,
                        broadcast: broadcast_str,
                        targets,
                    };
                }

                if let Some((admin, admin_str)) = admin_stuff {
                    return Routing::ReturnConAdmin((res_str, admin, admin_str));
                }

                Routing::Return(res_str)
            },
            Err(_) => Routing::Return("Invalid!".to_string()),
        }
}