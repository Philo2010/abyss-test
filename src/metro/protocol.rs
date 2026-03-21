use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::package::{header::Stations, rebuilt::{self, Patch}};


#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub enum OptionalModelPatch {
    Defence(f32),
    Comment(String),
    Game(Patch)
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
#[ts(export)]
pub enum Packages {
    CreateNewGame(CreateNewGame),
    CreateNewGameReturn(CreateNewGameReturn),
    LoginUser(LoginUser), 
    LoginUserReturn(LoginUserReturn),
    LoginUserUpdateAdmin(LoginUserUpdateAdmin),
    StartGame(StartGame),
    StartGameReturn(StartGameReturn),
    UpdateValue(UpdateValue),
    UpdateInformAdmin(UpdateInformAdmin),
    SubmitScouter(SubmitScouter),
    ReturnForAdmin(ReturnForAdmin),
    InformGameEnd(InformGameEnd), //should not send
    InformGameSend(), 
    Nop,
    Error(String)
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct CreateNewGame { 
    //Nothing really 
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct CreateNewGameReturn {
    pub admin: Uuid,
    pub code: usize,
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct LoginUser {
    pub username: String,
    pub code: usize,
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct LoginUserReturn {
    pub uuid: Uuid,
    pub station: Stations,
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct LoginUserUpdateAdmin {
    pub uuid: Uuid,
    pub username: String,
    pub station: Stations
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct StartGame {
    pub admin: Uuid,
    pub code: usize,
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct StartGameReturn {
    pub worked: bool,
    //if the game is in a invaild state then it wont work
}

//High path
#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct UpdateValue {
    pub patch: OptionalModelPatch,
    pub code: usize,
    pub uuid: Uuid,
} 

//No return we don't care about droped packages
#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct UpdateInformAdmin {
    pub patch: OptionalModelPatch,
    pub uuid: Uuid,
}

#[derive(Serialize, Deserialize, Debug, Clone, ts_rs::TS)]
pub struct SubmitScouter {
    pub data: rebuilt::Model,
    pub defence: f32,
    pub comment: String,
    pub game_id: usize,
    pub user_uuid: Uuid
}

#[derive(Serialize, Deserialize, Debug, ts_rs::TS)]
pub struct ReturnForAdmin {
    //diff type as diff purpose
    pub data: rebuilt::Model,
    pub defence: f32,
    pub comment: String,
    pub game_id: usize,
    pub user_uuid: Uuid
}

#[derive(Serialize, Deserialize, Debug, Clone, ts_rs::TS)]
pub struct InformGameEnd {
    //For all users
    pub targets: Vec<Uuid>,
    //since is not being just sent to the admin or return, we need a defined target
}