use gettextrs::gettext;
use gtk::{glib, glib::clone, prelude::*};
use ksni::{TrayMethods, menu::StandardItem};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    APP_ID, APP_NAME, Application, Window, gettext_f, prelude::*, session_list::SessionInfo, spawn,
    spawn_tokio,
};

enum Command {
    Open,
    OpenWindow(u32),
    Refresh,
    Quit,
}

struct Tray {
    sender: mpsc::UnboundedSender<Command>,
    windows: Vec<(u32, String)>,
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        APP_ID.to_owned()
    }

    fn title(&self) -> String {
        APP_NAME.to_owned()
    }

    fn icon_name(&self) -> String {
        APP_ID.to_owned()
    }

    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }

    fn activate(&mut self, _: i32, _: i32) {
        let _ = self.sender.send(Command::Open);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut menu: Vec<_> = self
            .windows
            .iter()
            .map(|(id, label)| {
                let id = *id;
                StandardItem {
                    label: label.clone(),
                    activate: Box::new(move |tray: &mut Self| {
                        let _ = tray.sender.send(Command::OpenWindow(id));
                    }),
                    ..Default::default()
                }
                .into()
            })
            .collect();
        if menu.is_empty() {
            menu.push(
                StandardItem {
                    label: gettext_f("Open {account}", &[("account", APP_NAME)]),
                    activate: Box::new(|tray: &mut Self| tray.activate(0, 0)),
                    ..Default::default()
                }
                .into(),
            );
        }
        menu.push(
            StandardItem {
                label: gettext("Quit"),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(Command::Quit);
                }),
                ..Default::default()
            }
            .into(),
        );
        menu
    }
}

fn window_labels(app: &Application) -> Vec<(u32, String)> {
    let mut accounts: Vec<_> = app
        .windows()
        .into_iter()
        .filter_map(|window| window.downcast::<Window>().ok())
        .map(|window| {
            let account = window
                .session_selection()
                .selected_item()
                .and_downcast::<SessionInfo>()
                .map_or_else(
                    || APP_NAME.to_owned(),
                    |session| session.user_id().to_string(),
                );
            (window.id(), account)
        })
        .collect();
    accounts.sort_by_key(|(id, _)| *id);
    accounts
        .iter()
        .map(|(id, account)| {
            let mut label = gettext_f("Open {account}", &[("account", account)]);
            if accounts
                .iter()
                .filter(|(_, other)| other == account)
                .count()
                > 1
            {
                label = gettext_f(
                    "{account} (Window {number})",
                    &[("account", &label), ("number", &id.to_string())],
                );
            }
            (*id, label)
        })
        .collect()
}

pub(super) fn start(app: &Application) {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    app.connect_window_added(clone!(
        #[strong]
        sender,
        move |_, window| {
            if let Some(window) = window.downcast_ref::<Window>() {
                window
                    .session_selection()
                    .connect_selected_item_notify(clone!(
                        #[strong]
                        sender,
                        move |_| {
                            let _ = sender.send(Command::Refresh);
                        }
                    ));
            }
            let _ = sender.send(Command::Refresh);
        }
    ));
    app.connect_window_removed(clone!(
        #[strong]
        sender,
        move |_, _| {
            let _ = sender.send(Command::Refresh);
        }
    ));
    let _ = sender.send(Command::Refresh);
    let task = spawn_tokio!(async move {
        Tray {
            sender,
            windows: Vec::new(),
        }
        .disable_dbus_name(ashpd::is_sandboxed())
        .assume_sni_available(true)
        .spawn()
        .await
    });

    spawn!(clone!(
        #[weak]
        app,
        async move {
            let handle = match task.await.expect("task was not aborted") {
                Ok(handle) => handle,
                Err(error) => {
                    warn!("Could not start tray indicator: {error}");
                    return;
                }
            };
            app.connect_shutdown(clone!(
                #[strong]
                handle,
                move |_| {
                    handle.shutdown();
                }
            ));
            let app = app.downgrade();

            while let Some(command) = receiver.recv().await {
                let Some(app) = app.upgrade() else {
                    break;
                };
                match command {
                    Command::Open => app.activate(),
                    Command::OpenWindow(id) => {
                        if let Some(window) = app.window_by_id(id) {
                            window.present();
                        }
                    }
                    Command::Refresh => {
                        let windows = window_labels(&app);
                        handle.update(move |tray| tray.windows = windows).await;
                    }
                    Command::Quit => app.activate_action("quit", None),
                }
            }
        }
    ));
}
