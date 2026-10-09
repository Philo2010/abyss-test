pub mod header;
pub mod rebuilt;
pub mod storage;


pub struct Match {
    pub game: rebuilt::Model,
    pub header: header::Model,
    pub user_uuid: uuid::Uuid,
}
pub struct MatchOp {

}