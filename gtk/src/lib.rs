#[macro_use]
extern crate cascade;
#[macro_use]
extern crate derive_more;
#[macro_use]
extern crate derive_new;
#[macro_use]
extern crate enclose;
#[macro_use]
extern crate fomat_macros;
#[macro_use]
extern crate log;
#[macro_use]
extern crate shrinkwraprs;
#[macro_use]
extern crate thiserror;

mod battery;
mod errors;
mod events;
mod gtk_utils;
mod localize;
mod notify;
mod state;
mod users;
mod widgets;

pub use localize::localizer;

use self::{
    events::{BackgroundEvent, Event, EventWidgets},
    state::State,
    widgets::Section,
};
use gtk::prelude::*;
use std::{
    cell::RefCell,
    process::Command,
    rc::Rc,
    sync::{mpsc, Arc},
    thread,
};

const RECOVERY_PARTITION: usize = 0;
const REFRESH_OS: usize = 1;

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
        let (gui_sender, gui_receiver) = flume::unbounded();
        let gui_sender = Arc::new(gui_sender);

        thread::spawn(enclose!((gui_sender) move || {
            events::background::run(&bg_receiver, move |event| {
                let _ = gui_sender.send(event);
            });
        }));

        let button_sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
        let option_sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
        let sublab_sg = gtk::SizeGroup::new(gtk::SizeGroupMode::Both);

        let mut upgrade = Section::new(&fomat!("<b>" (fl!("os-upgrade")) "</b>"));
        let mut recovery = Section::new(&fomat!("<b>" (fl!("os-recovery")) "</b>"));

        upgrade.add_option(&option_sg, &button_sg, &sublab_sg, |option| {
            option.button_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);
        });

        recovery
            .add_option(&option_sg, &button_sg, &sublab_sg, |option| {
                option
                    .button_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION)
                    .label(&fl!("recovery-header"))
                    .sublabel(Some(&fl!("checking-for-updates")));
            })
            .add_option(&option_sg, &button_sg, &sublab_sg, |option| {
                option
                    .button_label(&fl!("button-refresh"))
                    .label(&fl!("refresh-header"))
                    .sublabel(Some(&fl!("refresh-description")));
            });

        let dismisser = gtk::ListBoxRow::new();
        upgrade.list.add(&dismisser);

        // let button_sg = cascade! {
        //     gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
        //     ..add_widget(&upgrade.options[0].button);
        //     ..add_widget(&recovery.options[REFRESH_OS].button);
        // };

        cascade! {
            gtk::SizeGroup::new(gtk::SizeGroupMode::Both);
            ..add_widget(upgrade.options[0].as_ref());
            ..add_widget(recovery.options[REFRESH_OS].as_ref());
        };

        let loading_label = gtk::Label::new(None);

        let loading = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 48);
            ..set_halign(gtk::Align::Center);
            ..set_valign(gtk::Align::Center);
            ..add(&loading_label);
            ..add(&cascade! {
                gtk::Spinner::new();
                ..set_size_request(128, 128);
                ..start();
            });
            ..show_all();
        };

        upgrade.frame.show_all();

        let container = cascade! {
            gtk::Box::new(gtk::Orientation::Vertical, 12);
            ..add(&upgrade.label);
            ..add(&upgrade.frame);
            ..add(&recovery.label);
            ..add(&recovery.frame);
            ..show_all();
        };

        let stack = cascade! {
            gtk::Stack::new();
            ..add_named(&container, "updated");
            ..add_named(&loading, "loading");
            ..set_visible_child_name("loading");
            ..show_all();
        };

        get_dismiss_row(&upgrade.list).hide();

        let callback_error: ErrorCallback = Rc::new(RefCell::new(Box::new(|_| ())));
        let callback_event: EventCallback = Rc::new(RefCell::new(Box::new(|_| ())));
        let callback_ready: ReadyCallback = Rc::new(RefCell::new(Box::new(|| ())));

        events::attach(
            gui_receiver,
            EventWidgets {
                button_sg,
                container,
                dismisser,
                loading_label,
                recovery,
                stack: stack.clone(),
                upgrade,
            },
            State::new(
                bg_sender.clone(),
                Arc::downgrade(&gui_sender),
                callback_error.clone(),
                callback_event.clone(),
                callback_ready.clone(),
            ),
        );

        Self {
            container: stack.upcast::<gtk::Container>(),
            sender: bg_sender,
            callback_error,
            callback_event,
            callback_ready,
        }
    }

    pub fn scan(&self) { let _ = self.sender.send(BackgroundEvent::Scan); }

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
