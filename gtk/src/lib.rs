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
    events::*,
    state::State,
    widgets::{UpgradeOption, UpgradeSection},
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
    #[allow(clippy::new_without_default)]
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

        let dismisser = gtk::ListBoxRow::new();

        let upgrade = cascade! {
            UpgradeSection::new(&fomat!("<b>" (fl!("os-upgrade")) "</b>"));
            ..add_option(cascade! {
                let option = UpgradeOption::new();
                ..button_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);
                button_sg.add_widget(&option.button);
            });
        };

        upgrade.list.add(&dismisser);
        upgrade.frame.show_all();

        let recovery = cascade! {
            UpgradeSection::new(&fomat!("<b>" (fl!("os-recovery")) "</b>"));
            ..add_option(cascade! {
                let option = UpgradeOption::new();
                ..button_class(&gtk::STYLE_CLASS_SUGGESTED_ACTION);
                ..label(&fl!("recovery-header"));
                ..sublabel(Some(&fl!("checking-for-updates")));
                button_sg.add_widget(&option.button);
            });
            ..add_option(cascade! {
                let option = UpgradeOption::new();
                ..button_label(&fl!("button-refresh"));
                ..label(&fl!("refresh-header"));
                ..sublabel(Some(&fl!("refresh-description")));
                button_sg.add_widget(&option.button);
            });
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

        let mut widgets = EventWidgets {
            button_sg,
            container,
            dismisser,
            loading_label,
            recovery,
            stack: stack.clone(),
            upgrade,
        };

        let mut state = State::new(
            bg_sender.clone(),
            Arc::downgrade(&gui_sender),
            callback_error.clone(),
            callback_event.clone(),
            callback_ready.clone(),
        );

        let widget = Self {
            container: stack.upcast::<gtk::Container>(),
            sender: bg_sender,
            callback_error,
            callback_event,
            callback_ready,
        };

        glib::MainContext::default().spawn_local(async move {
            while let Ok(event) = gui_receiver.recv_async().await {
                events::on_event(&mut widgets, &mut state, event).await;
            }
        });

        widget
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
    options.row_at_index(0).expect("upgrade option is not at index 0")
}

fn get_dismiss_row(options: &gtk::ListBox) -> gtk::ListBoxRow {
    options.row_at_index(1).expect("dismisser frame row is not at index 1")
}

fn reboot() { let _ = Command::new("systemctl").arg("reboot").status(); }
