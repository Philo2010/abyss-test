use std::collections::HashMap;
use std::{collections::HashSet, sync::Arc};

use std::sync::{Mutex, RwLock};


use uuid::Uuid;

use crate::package::rebuilt::{self, Patch, total_score_auto, total_score_teleop};
use crate::package::{header};
use crate::package::{Match, MatchOp, header::Stations};


#[derive(Clone)]
pub struct UserEntry {
    username: String,
    done: bool,
    station: Stations
}

pub enum GameState {
    Waiting,
    Active,
    Over, //Soon to be cleared
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

    pub fn get_admin(&self, id: usize) -> Uuid {
        let games = self.games.read().unwrap();
        let game = games.get(&id).unwrap();
        game.admin
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

    pub fn add_user(&self, id: usize, user: String, user_id: Uuid) -> Result<(Uuid, Stations), GameDataError> { //auto asign
        let avaable_stations = self.check_game(id)?;

        let curr_station: Stations;

        if let Some(station) = avaable_stations.first() {
            curr_station = *station;
        } else {
            //just pick a random one
            curr_station = match fastrand::u32(0..6) {
                0 => Stations::Red1,
                1 => Stations::Red2,
                2 => Stations::Red3,
                3 => Stations::Blue1,
                4 => Stations::Blue2,
                5 => Stations::Blue3,
                _ => unreachable!(),
            };
        }

        let user = UserEntry {
            station: curr_station,
            username: user,
            done: false
        };
        let games = self.games.read().unwrap();
        if let Some(matche) = games.get(&id) {
            let mut users = matche.users.write().unwrap();
            users.insert(user_id, user);
        } else {
            return Err(GameDataError::Invalid)
        }
        Ok((user_id, curr_station))
    }

    pub fn start_game(&self, id: usize) -> Result<(), GameDataError> {
        let issues = self.check_game(id)?;
        if !issues.is_empty() {
            return Err(GameDataError::GameIsNotReady);
        }

        let mut games= self.games.write().unwrap();
        let mut game: &mut Game = match games.get_mut(&id) {
            Some(a) => a,
            None => {
                return Err(GameDataError::InvalidID);
            },
        };
        game.state = GameState::Active;


        Ok(())
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

        //time to check if scouters are done
        for user in game.users.read().unwrap().iter() {
            if !user.1.done {
                return Err(GameDataError::GameIsNotReady);
            } else {
                users_send.push(*user.0);
            }
        }

        //we are now safe to end.... just one more safty check
        for finals in game.final_store.into_iter() {
            if finals.0.is_empty() {
                return Err(GameDataError::GameIsNotReady);
            }
        }
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


        //need to write now
        let mut games = self.games.write().unwrap();

        let mut game: &mut Game = games.get_mut(&game_id)
            .ok_or(GameDataError::InvalidID)?;
        let mut user_mods = game.users.write().unwrap();
        let mut user_mod =  user_mods.get_mut(&user_uuid).ok_or(GameDataError::InvalidID)?;
        user_mod.done = true;
        match user.station {
            Stations::Red1 => {
                game.final_store.red1.push(Match { header, game: data });
            },
            Stations::Red2 => {
                game.final_store.red2.push(Match { header, game: data });
            },
            Stations::Red3 => {
                game.final_store.red3.push(Match { header, game: data });
            },
            Stations::Blue1 => {
                game.final_store.blue1.push(Match { header, game: data });
            },
            Stations::Blue2 => {
                game.final_store.blue2.push(Match { header, game: data });
            },
            Stations::Blue3 => {
                game.final_store.blue3.push(Match { header, game: data });
            },
        }
        //Todo make the end game rotine
        match self.end_game(game_id) {
            Ok(a) => {
                return Ok(Some(a))
            },
            Err(a) => {
                return Ok(None);
            },
        };
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