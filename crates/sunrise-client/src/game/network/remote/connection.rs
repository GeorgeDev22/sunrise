//! The window's connection to a server, as the engine sees it: its record of the last server, the
//! hooks it calls as a connection starts, lets the player in and closes (`sunrise_hooks::connection`),
//! and the Disconnected screen's offer to connect again, which a crate above asks for
//! (`Hooks::retry`). With nothing asking, the screen is vanilla's `DisconnectedScreen`.
//!
//! A connection over the network is one session, from `connection_starts` until it closes, once,
//! in `connection_left` or `connection_ended`, whichever way: the player leaving, the game closing,
//! a transfer, the server, the connection or this client ending it. A world the window plays
//! itself never opens one.
use super::*;
use sunrise_client::{multiplayer, title::Mode};
use sunrise_hooks::connection::{By, Ended, Left, Retry, Server};

impl Game<'_> {
    /// A connection to `address` as `name` starts over the network (`Hooks::connecting`). One
    /// still open closes first, as the player leaving it.
    pub(in crate::game) fn connection_starts(&mut self, address: &str, name: &str, lan: bool) {
        self.connection_left(Left::Disconnected);
        self.pending_retry = None;
        self.retry_due = None;
        let server = Server::new(address, name, lan);
        sunrise_hooks::engine::connecting(&server);
        self.last_connection = Some(server);
        self.connection_open = true;
    }
    /// The server this window connected to last over the network: while connected, and after a
    /// lost connection while the Disconnected screen may connect there again.
    pub(in crate::game) fn server_address(&self) -> Option<&str> {
        let server = self.last_connection.as_ref();
        server.map(|server| server.address.as_str())
    }
    /// That server was found on the local network.
    pub(in crate::game) fn server_is_lan(&self) -> bool {
        self.last_connection
            .as_ref()
            .is_some_and(|server| server.lan)
    }
    /// The server let the player in (`Hooks::joined`); nothing for a world the window plays
    /// itself, whose session joins the same way.
    pub(in crate::game) fn connection_joined(&mut self) {
        if !self.connection_open {
            return;
        }
        sunrise_hooks::engine::joined();
    }
    /// The open network connection closes without being lost (`Hooks::left`). Nothing when
    /// none is open: it closed already, or the window plays its own world.
    pub(in crate::game) fn connection_left(&mut self, how: Left) {
        if !std::mem::take(&mut self.connection_open) {
            return;
        }
        self.last_connection = None;
        sunrise_hooks::engine::left(how);
    }
    /// The open network connection ended without the player leaving (`Hooks::ended`), before
    /// the Disconnected screen shows; nothing when none is open. The crates are asked once
    /// whether to connect again (`Hooks::retry`), and that screen acts on the answer
    /// (`offer_retry`); the last server is forgotten unless something will connect there again.
    pub(in crate::game) fn connection_ended(&mut self, reason: &str, key: Option<&str>, by: By) {
        if !std::mem::take(&mut self.connection_open) {
            return;
        }
        sunrise_hooks::engine::ended(&Ended::new(reason, key, by));
        self.pending_retry = sunrise_hooks::engine::retry();
        if self.pending_retry.is_none() {
            self.last_connection = None;
        }
    }
    /// The Disconnected screen just shown offers to connect again when the answer to the
    /// connection's end asked to: its line, or the connection on the next frame (`follow_retry`)
    /// when it is due at once.
    pub(in crate::game) fn offer_retry(&mut self) {
        let Some(answer) = self.pending_retry.take() else {
            return;
        };
        if retry_slot(&mut self.title).is_none() {
            return;
        }
        let line = match answer {
            Retry::Wait(line) => line,
            Retry::Now(server) => {
                self.retry_due = Some(server);
                String::new()
            }
            // An answer this engine doesn't know: no offer.
            _ => return,
        };
        if let Some(server) = &self.last_connection {
            let address = sunrise_client::net::shown_address(&server.address);
            log::info!("Sunrise: offering to connect to {address} again: {line}");
        }
        if let Some(slot) = retry_slot(&mut self.title) {
            *slot = Some(line);
        }
    }
    /// Once a frame while the Disconnected screen offers to connect again: the connection due
    /// from the last answer, else the crates' answer now (its line, the connection, or nothing
    /// any more). Cancelled or left, the offer and its due connection go. An online-mode server
    /// gets the signed-in account again: `connect_server` takes it from `auth::current()`, held
    /// in memory for the whole run (the launcher's session file is gone after the first read).
    pub(in crate::game) fn follow_retry(&mut self) -> Result<()> {
        if retry_slot(&mut self.title).is_none_or(|slot| slot.is_none()) {
            self.retry_due = None;
            return Ok(());
        }
        let answer = match self.retry_due.take() {
            Some(server) => Some(Retry::Now(server)),
            None => sunrise_hooks::engine::retry(),
        };
        let Some(slot) = retry_slot(&mut self.title) else {
            return Ok(());
        };
        let server = match answer {
            Some(Retry::Wait(line)) => {
                *slot = Some(line);
                return Ok(());
            }
            Some(Retry::Now(server)) => server,
            // Nothing asks any more.
            _ => {
                *slot = None;
                return Ok(());
            }
        };
        *slot = None;
        let realm = self.realm_session && self.server_address() == Some(server.address.as_str());
        self.connect_server_with_options(&server.address, &server.name, None, server.lan, realm)
    }
}

/// The Disconnected screen's line about connecting again, while that screen shows.
fn retry_slot(title: &mut Option<sunrise_client::title::Title>) -> Option<&mut Option<String>> {
    match &mut title.as_mut()?.mode {
        Mode::Multiplayer(multiplayer::Screen::Disconnected { retry, .. }) => Some(retry),
        _ => None,
    }
}
