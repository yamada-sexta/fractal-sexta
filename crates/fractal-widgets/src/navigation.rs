use gtk::{gio, prelude::*};

use crate::{
    intent::SessionIntent,
    prelude::*,
    session::{IdentityVerification, Room, Session},
    utils::matrix::MatrixIdUri,
};
pub fn show_matrix_id(session: &Session, uri: MatrixIdUri) -> bool {
    let Some(app) = gio::Application::default() else {
        return false;
    };
    let intent = SessionIntent::ShowMatrixId(uri);
    app.activate_action(
        SessionIntent::SHOW_MATRIX_ID_ACTION_NAME,
        Some(&intent.to_variant_with_session_id(session.session_id().to_owned())),
    );
    true
}
pub fn show_room(room: &Room) {
    if let Some(session) = room.session() {
        show_matrix_id(
            &session,
            MatrixIdUri::Room(room.room_id().to_owned().into()),
        );
    }
}
pub fn show_verification(verification: &IdentityVerification) {
    if let Some(app) = gio::Application::default() {
        let session = verification.user().session();
        let intent = SessionIntent::ShowIdentityVerification(verification.key());
        app.activate_action(
            SessionIntent::SHOW_IDENTITY_VERIFICATION_ACTION_NAME,
            Some(&intent.to_variant_with_session_id(session.session_id().to_owned())),
        );
    }
}
