use gettextrs::gettext;
use gtk::{glib, glib::clone, prelude::*};
use ksni::{TrayMethods, menu::StandardItem};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{APP_ID, APP_NAME, Application, spawn, spawn_tokio};

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Open,
    Quit,
}

struct Tray {
    sender: mpsc::UnboundedSender<Command>,
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
        vec![
            StandardItem {
                label: gettext("Open"),
                activate: Box::new(|tray: &mut Self| tray.activate(0, 0)),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: gettext("Quit"),
                activate: Box::new(|tray: &mut Self| {
                    let _ = tray.sender.send(Command::Quit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

pub(super) fn start(app: &Application) {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let task = spawn_tokio!(async move {
        Tray { sender }
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
            app.connect_shutdown(move |_| {
                handle.shutdown();
            });
            let app = app.downgrade();

            while let Some(command) = receiver.recv().await {
                let Some(app) = app.upgrade() else {
                    break;
                };
                match command {
                    Command::Open => app.activate(),
                    Command::Quit => app.activate_action("quit", None),
                }
            }
        }
    ));
}

#[cfg(test)]
mod tests {
    use ksni::Tray as _;

    use super::*;

    #[test]
    fn tray_actions_dispatch_commands() {
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let mut tray = Tray { sender };

        tray.activate(0, 0);
        assert_eq!(receiver.try_recv().unwrap(), Command::Open);

        let menu = tray.menu();
        assert_eq!(menu.len(), 2);
        for (item, command) in menu.into_iter().zip([Command::Open, Command::Quit]) {
            let ksni::MenuItem::Standard(item) = item else {
                panic!("expected a standard menu item");
            };
            (item.activate)(&mut tray);
            assert_eq!(receiver.try_recv().unwrap(), command);
        }
        assert!(receiver.try_recv().is_err());
    }
}
