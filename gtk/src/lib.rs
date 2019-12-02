#[macro_use]
extern crate cascade;
#[macro_use]
extern crate derive_new;
#[macro_use]
extern crate fomat_macros;
#[macro_use]
extern crate log;
#[macro_use]
extern crate shrinkwraprs;
#[macro_use]
extern crate thiserror;

mod errors;
mod events;
mod gtk_utils;
mod notify;
mod state;
mod users;
mod widgets;

use self::{events::*, state::State, widgets::Section};
use gtk::prelude::*;
use std::{
    cell::RefCell,
    process::Command,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
};

pub type ErrorCallback = Rc<RefCell<Box<dyn Fn(&str)>>>;
pub type EventCallback = Rc<RefCell<Box<dyn Fn(Event)>>>;
pub type ReadyCallback = Rc<RefCell<Box<dyn Fn()>>>;

#[derive(Shrinkwrap)]
pub struct UpgradeWidget {
    sender:         mpsc::SyncSender<BackgroundEvent>,
    callback_error: ErrorCallback,
    callback_event: EventCallback,
    callback_ready: ReadyCallback,
    #[shrinkwrap(main_field)]
    container:      gtk::Container,
}

impl UpgradeWidget {
    pub fn new() -> Self {
        let (bg_sender, bg_receiver) = mpsc::sync_channel(5);
        let (gui_sender, gui_receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        let gui_sender = Arc::new(gui_sender);

        {
            let gui_sender = gui_sender.clone();

            thread::spawn(move || {
                events::background::run(bg_receiver, move |event| {
                    let _ = gui_sender.send(event);
                });
            });
        }

        let upgrade = Section::new("<b>OS Upgrade</b>");
        let refresh = Section::new("<b>Refresh OS Install</b>");

        upgrade.option.button_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);

        refresh
            .option
            .button_label("Refresh")
            .label("Refresh OS")
            .sublabel(Some("Reinstall while keeping user accounts and files"));

        let dismisser = gtk::ListBoxRow::new();
        upgrade.list.add(&dismisser);

        let button_sg = cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(&upgrade.option.button);
            ..add_widget(&refresh.option.button);
        };

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(upgrade.option.as_ref());
            ..add_widget(refresh.option.as_ref());
        }

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 12);
            ..add(&upgrade.label);
            ..add(&upgrade.frame);
            ..add(&refresh.label);
            ..add(&refresh.frame);
            ..show_all();
        };

        get_dismiss_row(&upgrade.list).hide();

        let callback_error: ErrorCallback = Rc::new(RefCell::new(Box::new(|_| ())));
        let callback_event: EventCallback = Rc::new(RefCell::new(Box::new(|_| ())));
        let callback_ready: ReadyCallback = Rc::new(RefCell::new(Box::new(|| ())));

        events::attach(
            gui_receiver,
            EventWidgets { button_sg, container: container.clone(), dismisser, upgrade, refresh },
            State::new(
                bg_sender.clone(),
                Arc::downgrade(&gui_sender),
                callback_error.clone(),
                callback_event.clone(),
                callback_ready.clone(),
            ),
        );

        Self {
            container: container.upcast::<gtk::Container>(),
            sender: bg_sender,
            callback_error,
            callback_event,
            callback_ready,
        }
    }

    pub fn scan(&self) {
        self.hide();
        let _ = self.sender.send(BackgroundEvent::Scan);
    }

    pub fn shutdown(&self) { let _ = self.sender.send(BackgroundEvent::Shutdown); }

    pub fn callback_error<F: Fn(&str) + 'static>(&self, func: F) {
        *self.callback_error.borrow_mut() = Box::from(func);
    }

    pub fn callback_event<F: Fn(Event) + 'static>(&self, func: F) {
        *self.callback_event.borrow_mut() = Box::from(func);
    }

    pub fn callback_ready<F: Fn() + 'static>(&self, func: F) {
        *self.callback_ready.borrow_mut() = Box::from(func);
    }

    pub fn upgrade_daemon_is_active(&self) -> bool {
        let (tx, rx) = mpsc::sync_channel(0);
        let _ = self.sender.send(BackgroundEvent::IsActive(tx));
        rx.recv().unwrap_or(false)
    }
}

fn get_upgrade_row(options: &gtk::ListBox) -> gtk::ListBoxRow {
    options.get_row_at_index(0).expect("upgrade option is not at index 0")
}

fn get_dismiss_row(options: &gtk::ListBox) -> gtk::ListBoxRow {
    options.get_row_at_index(1).expect("dismisser frame row is not at index 1")
}

fn reboot() { let _ = Command::new("systemctl").arg("reboot").status(); }
