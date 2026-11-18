use std::{
    cell::{Cell, OnceCell, RefCell},
    sync::{Arc, atomic::{AtomicBool, Ordering}},
};

use adw::{prelude::*, subclass::prelude::*};
use gettextrs::gettext;
use gtk::{gio, glib, glib::clone};
use ruma::{OwnedEventId, UserId};
use tracing::error;

use crate::{
    gettext_f, ngettext_f, prelude::*,
    session::{Member, PresenceFilter, Room, RoomSearchFilter, RoomSearchSnapshot, SearchMessage},
    spawn, spawn_tokio, utils::matrix::timestamp_to_date,
};

struct SearchResult {
    message: Arc<SearchMessage>,
    excerpt: String,
}

mod imp {
    use super::*;

    #[derive(Default, gtk::CompositeTemplate)]
    #[template(resource = "/org/gnome/Fractal/ui/session_view/room_history/search.ui")]
    pub struct RoomSearchDialog {
        #[template_child]
        query: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        sender: TemplateChild<adw::EntryRow>,
        #[template_child]
        sender_suggestions: TemplateChild<gtk::ListBox>,
        #[template_child]
        filters: TemplateChild<adw::ExpanderRow>,
        #[template_child]
        results: TemplateChild<gtk::ListView>,
        #[template_child]
        status: TemplateChild<gtk::Label>,
        #[template_child]
        load_older: TemplateChild<gtk::Button>,
        #[template_child]
        refresh: TemplateChild<gtk::Button>,
        pub(super) room: OnceCell<Room>,
        snapshot: RefCell<Arc<RoomSearchSnapshot>>,
        model: OnceCell<gio::ListStore>,
        controls: OnceCell<Vec<adw::ComboRow>>,
        generation: Cell<u64>,
        pub(super) active: Cell<bool>,
        loading: Cell<bool>,
        reached_start: Cell<bool>,
        failed: Cell<bool>,
        task: RefCell<Option<tokio::task::AbortHandle>>,
        cancelled: RefCell<Option<Arc<AtomicBool>>>,
        pub(super) on_jump: OnceCell<Box<dyn Fn(OwnedEventId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for RoomSearchDialog {
        const NAME: &'static str = "RoomSearchDialog";
        type Type = super::RoomSearchDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            Self::bind_template(klass);
            Self::bind_template_callbacks(klass);
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for RoomSearchDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let mut controls = Vec::new();
            for (index, title) in [
                gettext("Links"), gettext("Attachments"), gettext("Images"),
                gettext("Videos"), gettext("Audio"), gettext("Documents / Files"),
            ].into_iter().enumerate() {
                let choices = if index < 2 {
                    [gettext("Any"), gettext("Has"), gettext("Does not have")]
                } else {
                    [gettext("Any"), gettext("Include"), gettext("Exclude")]
                };
                let row = adw::ComboRow::builder()
                    .title(title)
                    .model(&gtk::StringList::new(&[
                        &choices[0], &choices[1], &choices[2],
                    ]))
                    .build();
                row.connect_selected_notify(clone!(
                    #[weak(rename_to = obj)]
                    self.obj(),
                    move |_| obj.imp().search_changed()
                ));
                self.filters.add_row(&row);
                controls.push(row);
            }
            self.controls.set(controls).unwrap();
            self.results
                .set_model(Some(&gtk::NoSelection::new(Some(self.model().clone()))));
            let factory = gtk::SignalListItemFactory::new();
            factory.connect_setup(clone!(
                #[weak(rename_to = obj)]
                self.obj(),
                move |_, item| {
                    let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return; };
                    let row = gtk::Box::builder()
                        .orientation(gtk::Orientation::Vertical)
                        .spacing(6)
                        .margin_top(8).margin_bottom(8).margin_start(8).margin_end(8)
                        .build();
                    let heading = gtk::Label::builder()
                        .xalign(0.0).wrap(true)
                        .wrap_mode(gtk::pango::WrapMode::WordChar).build();
                    heading.add_css_class("heading");
                    row.append(&heading);
                    let preview = gtk::Label::builder()
                        .xalign(0.0).wrap(true)
                        .wrap_mode(gtk::pango::WrapMode::WordChar)
                        .lines(4).ellipsize(gtk::pango::EllipsizeMode::End)
                        .selectable(true).build();
                    row.append(&preview);
                    let jump = gtk::Button::builder()
                        .label(gettext("Jump to Message")).halign(gtk::Align::End).build();
                    jump.connect_clicked(clone!(
                        #[weak]
                        obj,
                        #[weak]
                        item,
                        move |_| {
                            let Some(value) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                                return;
                            };
                            let id = value.borrow::<SearchResult>().message.event_id.clone();
                            obj.imp().jump(id);
                        }
                    ));
                    row.append(&jump);
                    item.set_child(Some(&row));
                }
            ));
            factory.connect_bind(clone!(
                #[weak(rename_to = obj)]
                self.obj(),
                move |_, item| {
                    let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return; };
                    let Some(value) = item.item().and_downcast::<glib::BoxedAnyObject>() else {
                        return;
                    };
                    let Some(row) = item.child().and_downcast::<gtk::Box>() else { return; };
                    let Some(heading) = row.first_child().and_downcast::<gtk::Label>() else {
                        return;
                    };
                    let Some(preview) = heading.next_sibling().and_downcast::<gtk::Label>() else {
                        return;
                    };
                    let result = value.borrow::<SearchResult>();
                    let message = &result.message;
                    let sender = obj.imp().room.get().unwrap()
                        .get_or_create_members().get(&message.sender)
                        .map_or_else(|| message.sender.to_string(), |member| member.display_name());
                    let timestamp = timestamp_to_date(message.timestamp)
                        .format("%x %X").map(|s| s.to_string()).unwrap_or_default();
                    heading.set_label(&format!("{sender} · {timestamp}"));
                    heading.set_tooltip_text(Some(message.sender.as_str()));
                    preview.set_label(&result.excerpt);
                    if let Some(jump) = row.last_child().and_downcast::<gtk::Button>() {
                        jump.set_sensitive(true);
                    }
                }
            ));
            factory.connect_unbind(|_, item| {
                let Some(item) = item.downcast_ref::<gtk::ListItem>() else { return; };
                let Some(row) = item.child() else { return; };
                let mut child = row.first_child();
                while let Some(widget) = child {
                    if let Some(label) = widget.downcast_ref::<gtk::Label>() {
                        label.set_label("");
                        label.set_tooltip_text(None);
                    } else if let Some(button) = widget.downcast_ref::<gtk::Button>() {
                        button.set_sensitive(false);
                    }
                    child = widget.next_sibling();
                }
            });
            self.results.set_factory(Some(&factory));
            self.obj().connect_closed(|obj| {
                let imp = obj.imp();
                imp.active.set(false);
                imp.cancel();
                imp.set_loading(false);
            });
        }

        fn dispose(&self) {
            self.cancel();
        }
    }
    impl WidgetImpl for RoomSearchDialog {}
    impl AdwDialogImpl for RoomSearchDialog {}

    #[gtk::template_callbacks]
    impl RoomSearchDialog {
        fn model(&self) -> &gio::ListStore {
            self.model.get_or_init(gio::ListStore::new::<glib::BoxedAnyObject>)
        }

        fn cancel(&self) {
            self.generation.set(self.generation.get().wrapping_add(1));
            if let Some(cancelled) = self.cancelled.take() {
                cancelled.store(true, Ordering::Relaxed);
            }
            if let Some(task) = self.task.take() {
                task.abort();
            }
        }

        fn set_loading(&self, loading: bool) {
            self.loading.set(loading);
            self.refresh.set_sensitive(!loading);
            self.load_older.set_sensitive(!loading && !self.reached_start.get());
        }

        #[template_callback]
        fn search_changed(&self) {
            // A cache load finishes with the latest criteria, independent of its starting query.
            if !self.active.get() || self.loading.get() {
                return;
            }
            self.cancel();
            let generation = self.generation.get();
            let sender_text = self.sender.text();
            let sender = if sender_text.trim().is_empty() {
                None
            } else {
                let Ok(sender) = UserId::parse(sender_text.trim()) else {
                    self.model().remove_all();
                    self.status.set_label(&gettext("Choose a sender or enter a full Matrix user ID"));
                    return;
                };
                Some(sender)
            };
            let controls = self.controls.get().unwrap();
            let presence = |index: usize| match controls[index].selected() {
                1 => PresenceFilter::Include,
                2 => PresenceFilter::Exclude,
                _ => PresenceFilter::Any,
            };
            let filter = RoomSearchFilter {
                text: glib::casefold(self.query.text().trim()).to_string(),
                sender,
                links: presence(0),
                attachments: presence(1),
                kinds: [presence(2), presence(3), presence(4), presence(5)],
            };
            let snapshot = self.snapshot.borrow().clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            self.cancelled.replace(Some(cancelled.clone()));
            self.status.set_label(&gettext("Searching cached messages…"));
            let worker = crate::RUNTIME.spawn_blocking(move || {
                snapshot.messages.iter()
                    .take_while(|_| !cancelled.load(Ordering::Relaxed))
                    .filter(|message| filter.matches(message))
                    .map(|message| SearchResult {
                        message: message.clone(),
                        excerpt: message.excerpt(&filter.text),
                    })
                    .collect::<Vec<_>>()
            });
            self.task.replace(Some(worker.abort_handle()));
            spawn!(clone!(
                #[weak(rename_to = obj)]
                self.obj(),
                async move {
                    let result = worker.await;
                    let imp = obj.imp();
                    if !imp.active.get() || imp.generation.get() != generation { return; }
                    imp.task.take();
                    let messages = match result {
                        Ok(messages) => messages,
                        Err(error) => {
                            imp.status.set_label(&gettext("Could not search history. Try again."));
                            error!("Room history matching task failed: {error}");
                            return;
                        }
                    };
                    imp.model().remove_all();
                    let mut messages = messages.into_iter();
                    glib::idle_add_local(clone!(
                        #[weak]
                        obj,
                        #[upgrade_or]
                        glib::ControlFlow::Break,
                        move || {
                            let imp = obj.imp();
                            if !imp.active.get() || imp.generation.get() != generation {
                                return glib::ControlFlow::Break;
                            }
                            let values: Vec<_> = messages.by_ref().take(100)
                                .map(glib::BoxedAnyObject::new).collect();
                            imp.model().splice(imp.model().n_items(), 0, &values);
                            if messages.len() == 0 {
                                imp.update_status();
                                glib::ControlFlow::Break
                            } else {
                                glib::ControlFlow::Continue
                            }
                        }
                    ));
                }
            ));
        }

        fn update_status(&self) {
            let count = self.model().n_items();
            let mut status = if count == 0 {
                if self.reached_start.get() {
                    gettext("No matches in accessible history")
                } else {
                    gettext("No cached matches. Load older history to search further.")
                }
            } else {
                ngettext_f("{n} match", "{n} matches", count, &[("n", &count.to_string())])
            };
            if count > 0 {
                status.push_str(" · ");
                status.push_str(&if self.reached_start.get() {
                    gettext("All accessible history searched")
                } else {
                    gettext("Cached history only")
                });
            }
            let undecryptable = self.snapshot.borrow().undecryptable;
            if undecryptable > 0 {
                status.push_str(&gettext_f(
                    " · {n} encrypted messages could not be searched",
                    &[("n", &undecryptable.to_string())],
                ));
            }
            if self.failed.get() {
                status.push_str(&gettext(" · Could not load history. Try again."));
            }
            self.status.set_label(&status);
        }

        #[template_callback]
        fn sender_changed(&self) {
            while let Some(child) = self.sender_suggestions.first_child() {
                self.sender_suggestions.remove(&child);
            }
            let text = self.sender.text();
            let term = glib::casefold(text.trim());
            if !term.is_empty() && UserId::parse(text.trim()).is_err()
                && let Some(room) = self.room.get()
            {
                for member in room.get_or_create_members().iter::<Member>()
                    .filter_map(Result::ok)
                    .filter(|member| {
                        glib::casefold(format!("{} {}", member.display_name(), member.user_id()))
                            .contains(term.as_str())
                    }).take(8)
                {
                    let id = member.user_id().to_string();
                    let button = gtk::Button::builder()
                        .label(format!("{} ({id})", member.display_name()))
                        .has_frame(false).build();
                    button.set_tooltip_text(Some(&id));
                    if let Some(label) = button.child().and_downcast::<gtk::Label>() {
                        label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                        label.set_max_width_chars(36);
                    }
                    button.connect_clicked(clone!(
                        #[weak(rename_to = obj)]
                        self.obj(),
                        move |_| obj.imp().sender.set_text(&id)
                    ));
                    self.sender_suggestions.append(&button);
                }
            }
            self.sender_suggestions.set_visible(self.sender_suggestions.first_child().is_some());
            self.search_changed();
        }

        #[template_callback]
        pub(super) fn reload_cache(&self) {
            self.load(false);
        }

        #[template_callback]
        fn load_older(&self) {
            self.load(true);
        }

        fn load(&self, older: bool) {
            if self.loading.get() || !self.active.get() { return; }
            self.cancel();
            self.set_loading(true);
            self.status.set_label(&if older {
                gettext("Loading older history…")
            } else {
                gettext("Reading cached history…")
            });
            let generation = self.generation.get();
            let room = self.room.get().unwrap().matrix_room().clone();
            let previous = self.snapshot.borrow().clone();
            let cancelled = Arc::new(AtomicBool::new(false));
            self.cancelled.replace(Some(cancelled.clone()));
            let worker = spawn_tokio!(async move {
                let pagination_result = if older {
                    RoomSearchSnapshot::load_older(&room, &previous.event_ids, &cancelled).await
                } else {
                    room.event_cache().await.map(|(cache, _handles)| {
                        matches!(cache.pagination().status().get(),
                            matrix_sdk::event_cache::PaginationStatus::Idle { hit_timeline_start: true })
                    }).map_err(|error| error.to_string())
                };
                let snapshot = RoomSearchSnapshot::load(&room, cancelled).await;
                (pagination_result, snapshot)
            });
            self.task.replace(Some(worker.abort_handle()));
            spawn!(clone!(
                #[weak(rename_to = obj)]
                self.obj(),
                async move {
                    let result = worker.await;
                    let imp = obj.imp();
                    if !imp.active.get() || imp.generation.get() != generation { return; }
                    imp.task.take();
                    let (pagination, snapshot) = match result {
                        Ok(result) => result,
                        Err(error) => {
                            imp.failed.set(true);
                            imp.reached_start.set(false);
                            imp.set_loading(false);
                            imp.update_status();
                            error!("Room history cache task failed: {error}");
                            return;
                        }
                    };
                    imp.failed.set(pagination.is_err() || snapshot.is_err());
                    match pagination {
                        Ok(reached_start) => imp.reached_start.set(reached_start && !imp.failed.get()),
                        Err(error) => {
                            imp.reached_start.set(false);
                            error!("Could not paginate room search history: {error}");
                        }
                    }
                    match snapshot {
                        Ok(snapshot) => { imp.snapshot.replace(Arc::new(snapshot)); }
                        Err(error) => error!("Could not read room search cache: {error}"),
                    }
                    imp.set_loading(false);
                    imp.search_changed();
                }
            ));
        }

        #[template_callback]
        fn activate_result(&self, position: u32) {
            if let Some(value) = self.model().item(position).and_downcast::<glib::BoxedAnyObject>() {
                self.jump(value.borrow::<SearchResult>().message.event_id.clone());
            }
        }

        fn jump(&self, id: OwnedEventId) {
            if let Some(on_jump) = self.on_jump.get() {
                on_jump(id);
            }
            self.obj().close();
        }

        pub(super) fn focus_query(&self) {
            self.query.grab_focus();
        }
    }
}

glib::wrapper! {
    pub struct RoomSearchDialog(ObjectSubclass<imp::RoomSearchDialog>)
        @extends gtk::Widget, adw::Dialog,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl RoomSearchDialog {
    pub(super) fn new(room: &Room, on_jump: impl Fn(OwnedEventId) + 'static) -> Self {
        let obj: Self = glib::Object::new();
        obj.imp().room.set(room.clone()).unwrap();
        obj.imp().on_jump.set(Box::new(on_jump)).ok().unwrap();
        obj
    }

    pub(super) fn is_search_active(&self) -> bool {
        self.imp().active.get()
    }

    pub(super) fn open(&self, parent: &impl IsA<gtk::Widget>) {
        self.imp().active.set(true);
        self.present(Some(parent));
        self.imp().focus_query();
        self.imp().reload_cache();
    }
}
