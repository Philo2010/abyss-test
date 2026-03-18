use std::sync::{Arc, RwLock};

use uuid::Uuid;

use crate::{package::storage::{GameDataError, Storage}, metro::protocol::{CreateNewGameReturn, InformGameEnd, LoginUserReturn, LoginUserUpdateAdmin, Packages, ReturnForAdmin, StartGameReturn, UpdateInformAdmin}};


//Todo: make admin checking (im not joking that is how little time i have to make this)

//The option is to send a package to admin (not used if admin is the resv)
pub fn handle_pkg(game: &Storage, message: Packages, uuid: Uuid) -> Result<(Packages, Option<(Packages, Uuid)>), GameDataError> {
    match message {
        Packages::CreateNewGame(create_new_game) => {
            let comp = game.create_new_game(uuid);
            let uuid = uuid;
            let res = CreateNewGameReturn {
                admin: uuid,
                code: comp,
            };
            return Ok((Packages::CreateNewGameReturn(res), None));
        },
        Packages::CreateNewGameReturn(create_new_game_return) => {
            return Ok((Packages::Nop, None)); //Should never send this package
        },
        Packages::LoginUser(login_user) => {
            let comp = game.add_user(login_user.code, login_user.username.clone(), uuid)?;

            let res = LoginUserReturn {
                uuid: comp.0,
                station: comp.1,
            };
            let res_for_admin = LoginUserUpdateAdmin {
                uuid: comp.0,
                username: login_user.username,
                station: comp.1,
            };
            let admin = game.get_admin(login_user.code);
            Ok((Packages::LoginUserReturn(res), Some((Packages::LoginUserUpdateAdmin(res_for_admin), admin))))
        },
        Packages::LoginUserReturn(login_user_return) => {
            Ok((Packages::Nop, None))
        },
        Packages::LoginUserUpdateAdmin(login_user_update_admin) => {
            Ok((Packages::Nop, None))
        },
        Packages::StartGame(start_game) => {
            let comp = game.start_game(start_game.code)?;
            Ok(((Packages::StartGameReturn(StartGameReturn {worked: true})), None))
        },
        Packages::StartGameReturn(start_game_return) => {
            Ok((Packages::Nop, None))
        },
        Packages::UpdateValue(update_value) => {
            let res_for_admin = UpdateInformAdmin {
                patch: update_value.patch,
                uuid: update_value.uuid,
            };
            let admin = game.get_admin(update_value.code);

            Ok((Packages::Nop, Some((Packages::UpdateInformAdmin(res_for_admin), admin))))
        },
        Packages::UpdateInformAdmin(update_inform_admin) => {
            Ok((Packages::Nop, None))
        },
        Packages::SubmitScouter(submit_scouter) => {
            let comp = game.submit_value(submit_scouter.data.clone(), submit_scouter.defence, submit_scouter.comment.clone(), submit_scouter.game_id, submit_scouter.user_uuid)?;

            let res_for_admin = ReturnForAdmin {
                data: submit_scouter.data,
                defence: submit_scouter.defence,
                comment: submit_scouter.comment,
                game_id: submit_scouter.game_id,
                user_uuid: submit_scouter.user_uuid,
            };

            let res;
            match comp {
                Some(a) => {
                    res = Packages::InformGameEnd(InformGameEnd {
                        targets: a,
                    });
                    //game is dead now, kill it
                    game.kill_game(submit_scouter.game_id);
                },
                None => {
                    res = Packages::Nop;
                },
            }
            let admin = game.get_admin(submit_scouter.game_id);

            Ok((res, Some((Packages::ReturnForAdmin(res_for_admin), admin))))
        },
        Packages::ReturnForAdmin(return_for_admin) => {
            Ok((Packages::Nop, None))
        },
        Packages::InformGameEnd(inform_game_end) => {
            Ok((Packages::Nop, None))
        },
        Packages::Nop => {
            //Nop, ment for the sending side, so no need to impl
            Ok((Packages::Nop, None))
        },
        Packages::Error(_) => {
            Ok((Packages::Nop, None))
        },
        Packages::InformGameSend() => {
            Ok((Packages::Nop, None))
        },
    }
}