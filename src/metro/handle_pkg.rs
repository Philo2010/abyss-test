use uuid::Uuid;

use crate::{package::storage::{GameDataError, Storage}, metro::protocol::{CreateNewGameReturn, InformGameEnd, LoginUserReturn, LoginUserUpdateAdmin, Packages, ReturnForAdmin, StartGameReturn, UpdateInformAdmin}};

// Third element: optional broadcast — (package to send, list of target UUIDs)
pub fn handle_pkg(game: &Storage, message: Packages, uuid: Uuid) -> Result<(Packages, Option<(Packages, Uuid)>, Option<(Packages, Vec<Uuid>)>), GameDataError> {
    match message {
        Packages::CreateNewGame(_) => {
            let comp = game.create_new_game(uuid);
            let res = CreateNewGameReturn { admin: uuid, code: comp };
            Ok((Packages::CreateNewGameReturn(res), None, None))
        },
        Packages::CreateNewGameReturn(_) => Ok((Packages::Nop, None, None)),
        Packages::LoginUser(login_user) => {
            let comp = game.add_user(login_user.code, login_user.username.clone(), uuid)?;
            let res = LoginUserReturn { uuid: comp.0, station: comp.1 };
            let res_for_admin = LoginUserUpdateAdmin {
                uuid: comp.0,
                username: login_user.username,
                station: comp.1,
            };
            let admin = game.get_admin(login_user.code);
            Ok((Packages::LoginUserReturn(res), Some((Packages::LoginUserUpdateAdmin(res_for_admin), admin)), None))
        },
        Packages::LoginUserReturn(_) => Ok((Packages::Nop, None, None)),
        Packages::LoginUserUpdateAdmin(_) => Ok((Packages::Nop, None, None)),
        Packages::StartGame(start_game) => {
            let user_ids = game.start_game(start_game.code)?;
            Ok((Packages::StartGameReturn(StartGameReturn { worked: true }), None, Some((Packages::InformGameSend(), user_ids))))
        },
        Packages::StartGameReturn(_) => Ok((Packages::Nop, None, None)),
        Packages::UpdateValue(update_value) => {
            let res_for_admin = UpdateInformAdmin {
                patch: update_value.patch,
                uuid: update_value.uuid,
            };
            let admin = game.get_admin(update_value.code);
            Ok((Packages::Nop, Some((Packages::UpdateInformAdmin(res_for_admin), admin)), None))
        },
        Packages::UpdateInformAdmin(_) => Ok((Packages::Nop, None, None)),
        Packages::SubmitScouter(submit_scouter) => {
            let comp = game.submit_value(submit_scouter.data.clone(), submit_scouter.defence, submit_scouter.comment.clone(), submit_scouter.game_id, submit_scouter.user_uuid)?;
            let res_for_admin = ReturnForAdmin {
                data: submit_scouter.data,
                defence: submit_scouter.defence,
                comment: submit_scouter.comment,
                game_id: submit_scouter.game_id,
                user_uuid: submit_scouter.user_uuid,
            };
            let admin = game.get_admin(submit_scouter.game_id);
            let broadcast = match comp {
                Some(targets) => {
                    game.kill_game(submit_scouter.game_id);
                    Some((Packages::InformGameEnd(InformGameEnd { targets: targets.clone() }), targets))
                },
                None => None,
            };
            Ok((Packages::Nop, Some((Packages::ReturnForAdmin(res_for_admin), admin)), broadcast))
        },
        Packages::ReturnForAdmin(_) => Ok((Packages::Nop, None, None)),
        Packages::InformGameEnd(_) => Ok((Packages::Nop, None, None)),
        Packages::Nop => Ok((Packages::Nop, None, None)),
        Packages::Error(_) => Ok((Packages::Nop, None, None)),
        Packages::InformGameSend() => Ok((Packages::Nop, None, None)),
    }
}
