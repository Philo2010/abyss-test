use serde::{Deserialize, Serialize};



#[derive(Clone, Copy, Eq, Hash, PartialEq, Serialize, Deserialize, ts_rs::TS)]
pub enum Stations {
    Red1,
    Red2,
    Red3,
    Blue1,
    Blue2,
    Blue3,
}


pub struct Model {
    pub user: String,
    pub total_score: f32,
    pub teleop_score: f32,
    pub auto_score: f32,
    pub defence: f32,
    pub comment: String,
    pub station: Stations,
}

pub struct ModelOptional {
    pub user: Option<String>,
    pub total_score: Option<f32>,
    pub teleop_score: Option<f32>,
    pub auto_score: Option<f32>,
    pub defence: Option<f32>,
    pub comment: Option<String>,
    pub station: Option<Stations>,
}