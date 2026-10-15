use std::cell::Cell;

use gtk::{gio, glib, glib::clone, prelude::*, subclass::prelude::*};

use crate::utils::BoundObject;

const INITIAL_ITEMS: u32 = 40;
const PAGE_SIZE: u32 = 20;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct HistoryModel {
        pub(super) model: BoundObject<gio::ListModel>,
        pub(super) offset: Cell<u32>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HistoryModel {
        const NAME: &'static str = "RoomHistoryModel";
        type Type = super::HistoryModel;
        type Interfaces = (gio::ListModel,);
    }

    impl ObjectImpl for HistoryModel {
        fn dispose(&self) {
            self.model.disconnect_signals();
        }
    }

    impl ListModelImpl for HistoryModel {
        fn item_type(&self) -> glib::Type {
            glib::Object::static_type()
        }

        fn n_items(&self) -> u32 {
            self.model
                .obj()
                .map_or(0, |model| model.n_items() - self.offset.get())
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            if position >= self.n_items() {
                return None;
            }
            self.model.obj()?.item(position + self.offset.get())
        }
    }

    impl HistoryModel {
        pub(super) fn items_changed(
            &self,
            model: &gio::ListModel,
            position: u32,
            removed: u32,
            added: u32,
        ) {
            let offset = self.offset.get();
            let before = model.n_items() - added + removed;
            let visible = before - offset;
            if position == 0 && removed == before {
                self.offset
                    .set(model.n_items().saturating_sub(visible.max(INITIAL_ITEMS)));
                self.obj().items_changed(0, visible, self.n_items());
            } else if offset > 0 && position + removed <= offset {
                // Track hidden prepends without replacing any visible rows.
                self.offset.set(offset - removed + added);
            } else if position >= offset {
                self.obj().items_changed(position - offset, removed, added);
            } else {
                self.offset.set(position);
                self.obj()
                    .items_changed(0, position + removed - offset, added);
            }
            debug_assert!(self.offset.get() <= model.n_items());
        }
    }
}

glib::wrapper! {
    /// A tail of the timeline that grows by prepending cached history on demand.
    pub struct HistoryModel(ObjectSubclass<imp::HistoryModel>)
        @implements gio::ListModel;
}

impl HistoryModel {
    pub(super) fn new() -> Self {
        glib::Object::new()
    }

    pub(super) fn set_model(&self, model: Option<gio::ListModel>) {
        let imp = self.imp();
        let removed = self.n_items();
        imp.model.disconnect_signals();
        imp.offset.set(0);
        if let Some(model) = model {
            imp.offset
                .set(model.n_items().saturating_sub(INITIAL_ITEMS));
            let handler = model.connect_items_changed(clone!(
                #[weak(rename_to = obj)]
                self,
                move |model, position, removed, added| {
                    obj.imp().items_changed(model, position, removed, added);
                }
            ));
            imp.model.set(model, vec![handler]);
        }
        self.items_changed(0, removed, self.n_items());
    }

    pub(super) fn reveal_older(&self) -> bool {
        let offset = self.imp().offset.get();
        if offset == 0 {
            return false;
        }
        self.reveal_position(offset.saturating_sub(PAGE_SIZE));
        true
    }

    pub(super) fn reveal_position(&self, position: u32) -> u32 {
        let offset = self.imp().offset.get();
        if position < offset {
            self.imp().offset.set(position);
            self.items_changed(0, 0, offset - position);
        }
        position - self.imp().offset.get()
    }
}
