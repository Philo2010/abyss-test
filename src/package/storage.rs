use std::collections::HashMap;
use std::{collections::HashSet, sync::Arc};

use std::sync::{Mutex, RwLock};


use uuid::Uuid;

use crate::metro::protocol::OptionalModelPatch;
use crate::package::rebuilt::{self, Patch, total_score_auto, total_score_teleop};
use crate::package::{header};
use crate::package::{Match, header::Stations};


#[derive(Clone)]
pub struct UserEntry {
    username: String,
    done: bool,
    station: Stations,
    /// Latest un-submitted values, kept so the admin can be re-synced after a
    /// dropped connection (see [`Storage::snapshot`]).
    live: UserLiveState,
}

/// Accumulated live edits for a single scout. Patches are coalesced per field
/// so a long game does not grow this without bound.
#[derive(Clone, Default)]
pub struct UserLiveState {
    patches: Vec<OptionalModelPatch>,
}

impl UserLiveState {
    /// Store `patch`, replacing any previous patch that targets the same field.
    pub fn apply(&mut self, patch: OptionalModelPatch) {
        let key = patch_key(&patch);
        if let Some(slot) = self.patches.iter_mut().find(|p| patch_key(p) == key) {
            *slot = patch;
        } else {
            self.patches.push(patch);
        }
    }

    /// Coalesced patches, safe to replay to the admin.
    pub fn patches(&self) -> &[OptionalModelPatch] {
        &self.patches
    }
}

/// Discriminant used to coalesce patches that write the same field.
fn patch_key(patch: &OptionalModelPatch) -> u8 {
    match patch {
        OptionalModelPatch::Defence(_) => 0,
        OptionalModelPatch::Comment(_) => 1,
        OptionalModelPatch::Game(game) => match game {
            Patch::DefenceMain(_) => 2,
            Patch::FuelShootTeleop(_) => 3,
            Patch::FuelPassTeleop(_) => 4,
            Patch::FuelShootAuto(_) => 5,
            Patch::FuelPassAuto(_) => 6,
            Patch::ClimbEnd(_) => 7,
            Patch::ClimbAuto(_) => 8,
            Patch::BeachOnBump(_) => 9,
            Patch::DefenceTarget(_) => 10,
            Patch::AutoTime(_) => 11,
            Patch::Dead(_) => 12,
            Patch::Dnf(_) => 13,
        },
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GameState {
    Waiting,
    Active,
    Over, //Soon to be cleared
}

/// Everything needed to re-attach a dropped client to its former identity.
pub struct ResumeInfo {
    pub code: usize,
    pub station: Option<Stations>,
    pub is_admin: bool,
    pub started: bool,
    pub ended: bool,
}

/// A scout's last known state, used to rebuild the admin table on reconnect.
pub struct UserSnapshot {
    pub uuid: Uuid,
    pub username: String,
    pub station: Stations,
    pub live: Vec<OptionalModelPatch>,
}

/// A finished run, used to rebuild the admin table on reconnect.
pub struct SubmittedSnapshot {
    pub data: rebuilt::Model,
    pub defence: f32,
    pub comment: String,
    pub user_uuid: Uuid,
}

pub struct GameSnapshot {
    pub state: GameState,
    pub users: Vec<UserSnapshot>,
    pub submitted: Vec<SubmittedSnapshot>,
}

#[derive(Default)]
pub struct TotalOppMatches {
    pub red1: Vec<Match>,
    pub red2: Vec<Match>,
    pub red3: Vec<Match>,
    pub blue1: Vec<Match>,
    pub blue2: Vec<Match>,
    pub blue3: Vec<Match>,
}

impl<'a> IntoIterator for &'a TotalOppMatches {
    type Item = (&'a Vec<Match>, Stations);
    type IntoIter = std::vec::IntoIter<Self::Item>; // can't use std::slice::Iter easily with tuple

    fn into_iter(self) -> Self::IntoIter {
        let mut list: Vec<Self::Item> = Vec::new();

        list.push((&self.red1, Stations::Red1));
        list.push((&self.red2, Stations::Red2));
        list.push((&self.red3, Stations::Red3));
        list.push((&self.blue1, Stations::Blue1));
        list.push((&self.blue2, Stations::Blue2));
        list.push((&self.blue3, Stations::Blue3));

        list.into_iter()
    }
}


pub struct Game {
    pub admin: Uuid,
    pub users: Arc<RwLock<HashMap<Uuid, UserEntry>>>,
    pub state: GameState,
    pub final_store: TotalOppMatches
}

#[derive(Clone)]
pub struct Storage {
    pub games: Arc<RwLock<HashMap<usize, Game>>>,
}


pub struct CreateReturn {
    pub admin: Uuid,
    pub game_id: usize,
}


impl Storage {
    pub fn new() -> Self {
        Storage {
            games: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub fn kill_game(&self, id: usize) {
        let mut games = self.games.write().unwrap();
        games.remove(&id);
    }

    pub fn get_admin(&self, id: usize) -> Result<Uuid, GameDataError> {
        let games = self.games.read().unwrap();
        let game = games.get(&id).ok_or(GameDataError::InvalidID)?;
        Ok(game.admin)
    }

    /// Look up a UUID across every game and report the identity behind it.
    ///
    /// Used by the resume handshake: the server must find out which game a
    /// reconnecting client belonged to before it can re-attach the socket.
    pub fn resume_info(&self, uuid: Uuid) -> Result<ResumeInfo, GameDataError> {
        let games = self.games.read().unwrap();
        for (code, game) in games.iter() {
            if game.admin == uuid {
                return Ok(ResumeInfo {
                    code: *code,
                    station: None,
                    is_admin: true,
                    started: game.state != GameState::Waiting,
                    ended: game.state == GameState::Over,
                });
            }

            if let Some(user) = game.users.read().unwrap().get(&uuid) {
                return Ok(ResumeInfo {
                    code: *code,
                    station: Some(user.station),
                    is_admin: false,
                    started: game.state != GameState::Waiting,
                    ended: game.state == GameState::Over,
                });
            }
        }
        Err(GameDataError::InvalidID)
    }

    /// Record the latest live edit for a scout so it can be replayed to a
    /// reconnecting admin.
    pub fn record_live(&self, id: usize, user_uuid: Uuid, patch: OptionalModelPatch) -> Result<(), GameDataError> {
        let games = self.games.read().unwrap();
        let game = games.get(&id).ok_or(GameDataError::InvalidID)?;
        let mut users = game.users.write().unwrap();
        let user = users.get_mut(&user_uuid).ok_or(GameDataError::InvalidID)?;
        if !user.done {
            user.live.apply(patch);
        }
        Ok(())
    }

    /// Full state of a game, used to rebuild an admin UI after a reconnect.
    pub fn snapshot(&self, id: usize) -> Result<GameSnapshot, GameDataError> {
        let games = self.games.read().unwrap();
        let game = games.get(&id).ok_or(GameDataError::InvalidID)?;

        let users = game.users.read().unwrap();
        let user_snapshots: Vec<UserSnapshot> = users
            .iter()
            .map(|(uuid, user)| UserSnapshot {
                uuid: *uuid,
                username: user.username.clone(),
                station: user.station,
                live: user.live.patches().to_vec(),
            })
            .collect();
        drop(users);

        let mut submitted = Vec::new();
        for (matches, _) in game.final_store.into_iter() {
            for m in matches {
                submitted.push(SubmittedSnapshot {
                    data: m.game.clone(),
                    defence: m.header.defence,
                    comment: m.header.comment.clone(),
                    user_uuid: m.user_uuid,
                });
            }
        }

        Ok(GameSnapshot {
            state: game.state,
            users: user_snapshots,
            submitted,
        })
    }

    pub fn create_new_game(&self, admin: Uuid) -> usize {
        let id = fastrand::usize(0..9999);

        let res = Game {
            admin,
            users: Arc::new(RwLock::new(HashMap::new())),
            final_store: TotalOppMatches {..Default::default()},
            state: GameState::Waiting
        };

        let mut data = self.games.write().unwrap();

        data.insert(id, res);
        return id;
    }

    pub fn add_user(&self, id: usize, user: String, user_id: Uuid) -> Result<(Uuid, Stations), GameDataError> {
        let all_stations = [
            Stations::Red1, Stations::Red2, Stations::Red3,
            Stations::Blue1, Stations::Blue2, Stations::Blue3,
        ];

        let games = self.games.read().unwrap();
        let game = games.get(&id).ok_or(GameDataError::InvalidID)?;
        let mut users = game.users.write().unwrap();

        let taken: HashSet<Stations> = users.values().map(|u| u.station).collect();
        let curr_station = all_stations.iter()
            .find(|&&s| !taken.contains(&s))
            .copied()
            .unwrap_or_else(|| all_stations[fastrand::usize(0..6)]);

        users.insert(user_id, UserEntry { station: curr_station, username: user, done: false, live: UserLiveState::default() });
        Ok((user_id, curr_station))
    }

    pub fn start_game(&self, id: usize) -> Result<Vec<Uuid>, GameDataError> {
        let mut games = self.games.write().unwrap();
        let game = games.get_mut(&id).ok_or(GameDataError::InvalidID)?;
        game.state = GameState::Active;
        let user_ids = game.users.read().unwrap().keys().copied().collect();
        Ok(user_ids)
    }

  
    pub fn get_user(&self, game_id: usize, user_uuid: Uuid) -> Result<UserEntry, GameDataError> {
        let games = self.games.read().unwrap();

        let game = games.get(&game_id)
            .ok_or(GameDataError::InvalidID)?;

        let gam_user = game.users.read().unwrap();

        let user = gam_user.get(&user_uuid)
            .ok_or(GameDataError::InvalidID)?;

        Ok(user.clone())
    }

    pub fn end_game(&self, game_id: usize) -> Result<Vec<Uuid>, GameDataError> {
        let games = self.games.read().unwrap();
        let game = games.get(&game_id).ok_or(GameDataError::InvalidID)?;
        let mut users_send = Vec::new();

        // The game is ready to end once every registered scout has submitted.
        let users = game.users.read().unwrap();
        if users.is_empty() {
            return Err(GameDataError::GameIsNotReady);
        }
        for (uuid, user) in users.iter() {
            if !user.done {
                return Err(GameDataError::GameIsNotReady);
            }
            users_send.push(*uuid);
        }
        drop(users);
        drop(games);

        let mut games = self.games.write().unwrap();
        let game = games.get_mut(&game_id).ok_or(GameDataError::InvalidID)?;
        game.state = GameState::Over;

        //to be cleaned up soon
        Ok(users_send)
    }

    pub fn close_game(&self, game_id: usize) {
        let mut games = self.games.write().unwrap();
        games.remove(&game_id);
    }

    pub fn submit_value(&self, data: rebuilt::Model, defence: f32, comment: String, game_id: usize, user_uuid: Uuid) -> Result<Option<Vec<Uuid>>, GameDataError> {

        let auto_score = total_score_auto(&data);
        let teleop_score =total_score_teleop(&data);
        let total_score = auto_score + teleop_score;


        let user = self.get_user(game_id, user_uuid)?;


        let header = header::Model {
            user: user.username,
            total_score,
            teleop_score,
            auto_score,
            defence,
            comment,
            station: user.station,
        };


        // Record the submission. Scoped so every lock is released before the
        // readiness check below (which re-locks `games`).
        {
            let mut games = self.games.write().unwrap();

            let game: &mut Game = games.get_mut(&game_id)
                .ok_or(GameDataError::InvalidID)?;
            let mut user_mods = game.users.write().unwrap();
            let mut user_mod = user_mods.get_mut(&user_uuid).ok_or(GameDataError::InvalidID)?;
            user_mod.done = true;
            match user.station {
                Stations::Red1 => {
                    game.final_store.red1.push(Match { header, game: data, user_uuid });
                },
                Stations::Red2 => {
                    game.final_store.red2.push(Match { header, game: data, user_uuid });
                },
                Stations::Red3 => {
                    game.final_store.red3.push(Match { header, game: data, user_uuid });
                },
                Stations::Blue1 => {
                    game.final_store.blue1.push(Match { header, game: data, user_uuid });
                },
                Stations::Blue2 => {
                    game.final_store.blue2.push(Match { header, game: data, user_uuid });
                },
                Stations::Blue3 => {
                    game.final_store.blue3.push(Match { header, game: data, user_uuid });
                },
            }
        }

        // Every scout in — the game can end.
        match self.end_game(game_id) {
            Ok(a) => Ok(Some(a)),
            Err(_) => Ok(None),
        }
    }


    pub fn check_game(&self, id: usize) -> Result<Vec<Stations>, GameDataError> {
        let games = self.games.read().unwrap();
        let game = games.get(&id).ok_or(GameDataError::InvalidID)?;

        let users = game.users.read().unwrap();

        // List of stations with potential problems
        let mut problems = vec![
            Stations::Red1,
            Stations::Red2,
            Stations::Red3,
            Stations::Blue1,
            Stations::Blue2,
            Stations::Blue3,
        ];

        // Set of stations that are liked by users
        let mut like_everything = HashSet::new();
        like_everything.insert(Stations::Red1);
        like_everything.insert(Stations::Red2);
        like_everything.insert(Stations::Red3);
        like_everything.insert(Stations::Blue1);
        like_everything.insert(Stations::Blue2);
        like_everything.insert(Stations::Blue3);

        // Iterate over users and remove stations that are not liked
        for user in users.iter() {
            let station = user.1.station;
            if !like_everything.contains(&station) {
                // Retain only stations that are liked (don't remove them from the problems list)
                problems.retain(|&x| x != station);
            }
        }

        // Return the list of problems (stations not liked)
        Ok(problems)
    }
}

pub enum GameDataError {
    Invalid,
    InvalidID,
    GameIsNotReady
}