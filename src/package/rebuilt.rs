use serde::{Deserialize, Serialize};

const FUEL_TELEOP: f32 = 1.0;
const FUEL_AUTO: f32 = 1.0;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, ts_rs::TS)]
pub enum ClimbState {
    Nothing,
    Stage1,
    Stage2,
    Stage3,
}

const STAGE_1_TELEOP: i32 = 10;
const STAGE_2_TELEOP: i32 = 20;
const STAGE_3_TELEOP: i32 = 30;
const STAGE_1_AUTO: i32 = 15;
const STAGE_2_AUTO: i32 = 15;
const STAGE_3_AUTO: i32 = 15;


#[derive(Serialize, Deserialize, Debug, Clone, Copy, ts_rs::TS)]
pub struct Model {
    pub defence_main: bool, //X
    pub fuel_shoot_teleop: f32, //X
    pub fuel_pass_teleop: f32, //X
    pub fuel_shoot_auto: f32, //X
    pub fuel_pass_auto: f32, //X
    pub climb_end: ClimbState, 
    pub climb_auto: ClimbState, 
    pub beach_on_bump: bool, 
}

pub struct ModelOptional {
    pub defence_main: Option<bool>, //X
    pub fuel_shoot_teleop: Option<f32>, //X
    pub fuel_pass_teleop: Option<f32>, //X
    pub fuel_shoot_auto: Option<f32>, //X
    pub fuel_pass_auto: Option<f32>, //X
    pub climb_end: Option<ClimbState>, 
    pub climb_auto: Option<ClimbState>, 
    pub beach_on_bump: Option<bool>, 
}

#[derive(Serialize, Deserialize, Clone, Debug, Copy, ts_rs::TS)]
pub enum Patch {
    DefenceMain(bool),
    FuelShootTeleop(f32),
    FuelPassTeleop(f32),
    FuelShootAuto(f32),
    FuelPassAuto(f32),
    ClimbEnd(ClimbState),
    ClimbAuto(ClimbState),
    BeachOnBump(bool)
}


//impl

pub fn total_score_auto(insert_data: &Model) -> f32 {
    let fuel_auto = insert_data.fuel_shoot_auto * FUEL_AUTO;
    

    let climb_score: i32;
    match insert_data.climb_auto {
        ClimbState::Stage1 => {
            climb_score = STAGE_1_AUTO;
        },
        ClimbState::Stage2 => {
            climb_score = STAGE_2_AUTO;
        },
        ClimbState::Stage3 => {
            climb_score = STAGE_3_AUTO;
        },
        ClimbState::Nothing => {
            climb_score = 0;
        },
    }

    climb_score as f32 + fuel_auto
}

pub fn total_score_teleop(insert_data: &Model) -> f32 {
    let fuel_teleop = insert_data.fuel_shoot_teleop * FUEL_TELEOP;

    let climb_score: i32;
    match insert_data.climb_end {
        ClimbState::Stage1 => {
            climb_score = STAGE_1_TELEOP;
        },
        ClimbState::Stage2 => {
            climb_score = STAGE_2_TELEOP;
        },
        ClimbState::Stage3 => {
            climb_score = STAGE_3_TELEOP;
        },
        ClimbState::Nothing => {
            climb_score = 0;
        },
    }

    climb_score as f32 + fuel_teleop
}