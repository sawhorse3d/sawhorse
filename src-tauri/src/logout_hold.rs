//! The shell's side of the logout hold (#73, `sawhorse_logout_hold`): on Linux, a logout
//! inhibitor held with the session manager while the model has unsaved changes, so GNOME and
//! Cinnamon name Sawhorse and its unsaved work in their own logout dialog. On Windows and
//! macOS this does nothing: nothing is asked of either.
//!
//! The session bus calls are made on a thread of their own, in the order the page spoke, so a
//! session manager that is slow to answer never holds up the window. The bus connection is
//! kept for as long as Sawhorse runs: the session manager drops an inhibitor when the
//! connection that took it goes.
//!
//! `Inhibit` and `Uninhibit` are waited on for as long as the session manager takes. An
//! `Inhibit` given up on after a time limit could still be granted afterwards, with a cookie
//! the shell never heard, and that inhibitor would stay until Sawhorse ends, saved or not.
//! Waiting costs nothing but this thread: the page's later words queue behind the call, in
//! order, and closing the window or quitting never waits for it.

/// The logout hold, as the `close_guard` command sees it.
#[cfg_attr(not(target_os = "linux"), derive(Default))]
pub struct LogoutHold {
    #[cfg(target_os = "linux")]
    tell: Option<std::sync::mpsc::Sender<bool>>,
}

#[cfg(target_os = "linux")]
impl Default for LogoutHold {
    fn default() -> Self {
        let (tell, told) = std::sync::mpsc::channel();
        let started = std::thread::Builder::new()
            .name("logout-hold".into())
            .spawn(move || linux::work(told));
        // No thread, no hold: the window and its own close question are as they were.
        LogoutHold {
            tell: started.ok().map(|_| tell),
        }
    }
}

impl LogoutHold {
    /// The page's word on unsaved changes: true takes the hold, false gives it back. Never
    /// waits and never fails: a hold that cannot be taken leaves everything as it was.
    pub fn unsaved(&self, unsaved: bool) {
        #[cfg(target_os = "linux")]
        if let Some(tell) = &self.tell {
            let _ = tell.send(unsaved);
        }
        #[cfg(not(target_os = "linux"))]
        let _ = unsaved;
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::sync::mpsc::Receiver;

    use gio::prelude::*;
    use sawhorse_logout_hold::{
        Hold, Step, Took, APP_ID, BUS_NAME, INTERFACE, LOGOUT, OBJECT_PATH, REASON,
    };

    /// How long the bus itself may take to say whether a session manager is on it. Nothing
    /// is held by that call, so one given up on is only a take tried again the next time the
    /// model becomes unsaved (`Took::Failed`).
    const ASK_BUS_MS: i32 = 2_000;
    /// `Inhibit` and `Uninhibit` are waited on with no limit (GLib's `G_MAXINT`), so the
    /// shell always hears the cookie of an inhibitor it was given.
    const NO_LIMIT: i32 = i32::MAX;

    /// The thread's whole life: each word from the page, in order.
    pub fn work(told: Receiver<bool>) {
        let mut hold = Hold::new();
        let mut bus: Option<gio::DBusConnection> = None;
        for unsaved in told {
            let step = if unsaved {
                hold.unsaved()
            } else {
                hold.saved()
            };
            match step {
                Step::Nothing => {}
                Step::Take => hold.took(take(&mut bus)),
                Step::Release(cookie) => release(bus.as_ref(), cookie),
            }
        }
    }

    /// One call on the session bus, never one that starts a service.
    fn call(
        bus: &gio::DBusConnection,
        (name, path, interface): (&str, &str, &str),
        method: &str,
        parameters: &glib::Variant,
        timeout_ms: i32,
    ) -> Result<glib::Variant, glib::Error> {
        bus.call_sync(
            Some(name),
            path,
            interface,
            method,
            Some(parameters),
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            timeout_ms,
            gio::Cancellable::NONE,
        )
    }

    /// The session manager, as the calls below name it.
    const MANAGER: (&str, &str, &str) = (BUS_NAME, OBJECT_PATH, INTERFACE);
    /// The bus itself, asked who is on it.
    const BUS: (&str, &str, &str) = (
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    );

    /// Whether a session manager of this kind is on the bus. KDE Plasma has none: it closes
    /// each window at logout, so the window's own question shows there already.
    fn manager_present(bus: &gio::DBusConnection) -> Option<bool> {
        let asked = (BUS_NAME,).to_variant();
        let reply = call(bus, BUS, "NameHasOwner", &asked, ASK_BUS_MS).ok()?;
        reply.get::<(bool,)>().map(|(present,)| present)
    }

    /// `Inhibit(app_id, toplevel_xid, reason, flags)`: the cookie, or why there is none.
    fn take(bus: &mut Option<gio::DBusConnection>) -> Took {
        if bus.is_none() {
            match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
                Ok(found) => *bus = Some(found),
                Err(_) => return Took::Absent,
            }
        }
        let Some(bus) = bus.as_ref() else {
            return Took::Absent;
        };
        match manager_present(bus) {
            Some(true) => {}
            Some(false) => return Took::Absent,
            None => return Took::Failed,
        }
        let asked = (APP_ID, 0u32, REASON, LOGOUT).to_variant();
        match call(bus, MANAGER, "Inhibit", &asked, NO_LIMIT) {
            Ok(reply) => match reply.get::<(u32,)>() {
                Some((cookie,)) => Took::Held(cookie),
                None => Took::Failed,
            },
            Err(_) => Took::Failed,
        }
    }

    /// `Uninhibit(cookie)`. A call that fails leaves the inhibitor to go with the connection.
    fn release(bus: Option<&gio::DBusConnection>, cookie: u32) {
        if let Some(bus) = bus {
            let asked = (cookie,).to_variant();
            let _ = call(bus, MANAGER, "Uninhibit", &asked, NO_LIMIT);
        }
    }
}
