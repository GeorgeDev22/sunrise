//! Auto-reconnect (`autoReconnect`, docs/extras.md): after a lost multiplayer connection the
//! Disconnected screen counts down to a new connection to the same server, with backoff, and never
//! after a kick, a ban or a refusal ([`policy`]).
//!
//! A Sunrise crate (docs/layers.md): the game tells it about the window's connection through the
//! platform's hooks ([`HOOKS`]: `connecting`, `joined`, `left`, `ended`) and asks it whether to
//! connect again (`retry`); the engine draws the screen and makes the connection. The official
//! executable (`sunrise-official`) registers it with its switch: its line of `sunrise-extras.txt`
//! ([`SETTING`]) and its row in Extras' Multiplayer group ([`SECTION`]). sunrise-client's own
//! executable never reconnects by itself.
#![forbid(unsafe_code)]
mod policy;

use std::sync::Mutex;
use std::time::Instant;

use sunrise_platform::hooks::connection::{By, Ended, Left, Retry, Server};
use sunrise_platform::hooks::{self, Hooks};
use sunrise_platform::pages::{Act, Label, Page, Place, Row};
use sunrise_platform::settings::Store;

/// The setting's localized label and tooltip, registered only with the crate.
pub static LANG: &sunrise_platform::lang::Files = include!(concat!(env!("OUT_DIR"), "/locales.rs"));

/// Its hooks, for `sunrise_platform::hooks::register`.
pub static HOOKS: Reconnect = Reconnect;

pub struct Reconnect;

impl Hooks for Reconnect {
    fn connecting(&self, server: &Server) {
        with_seat(|seat| {
            seat.planned = None;
            let tracker = &mut seat.tracker;
            tracker.connecting(&server.address, &server.name, server.lan);
        });
    }

    fn joined(&self) {
        with_seat(|seat| seat.tracker.joined(Instant::now()));
    }

    fn left(&self, _how: Left) {
        with_seat(|seat| {
            seat.planned = None;
            seat.tracker.reset();
        });
    }

    /// The next attempt, while the switch is on and the reason allows one.
    fn ended(&self, end: &Ended<'_>) {
        with_seat(|seat| {
            seat.planned = if seat.on && end.by != By::Client {
                let tracker = &mut seat.tracker;
                tracker.ended(end.reason, end.key, end.by == By::Server, Instant::now())
            } else {
                seat.tracker.reset();
                None
            };
        });
    }

    /// The countdown to the next attempt, then the attempt.
    fn retry(&self) -> Option<Retry> {
        with_seat(|seat| {
            let planned = seat.planned.as_ref()?;
            let now = Instant::now();
            Some(if planned.due(now) {
                let (address, name) = (planned.address.clone(), planned.name.clone());
                Retry::Now(Server::new(address, name, planned.lan))
            } else {
                Retry::Wait(planned.text(now))
            })
        })
    }
}

/// One splitscreen player's auto-reconnect (`hooks::seat`): the switch, the attempts at their
/// server and the next one.
#[derive(Default)]
struct Seat {
    on: bool,
    tracker: policy::Tracker,
    planned: Option<policy::Retry>,
}

static SEATS: Mutex<Vec<Seat>> = Mutex::new(Vec::new());

/// The player whose turn it is.
fn with_seat<T>(f: impl FnOnce(&mut Seat) -> T) -> T {
    let mut seats = SEATS.lock().unwrap_or_else(|e| e.into_inner());
    let seat = hooks::seat();
    if seats.len() <= seat {
        seats.resize_with(seat + 1, Seat::default);
    }
    f(&mut seats[seat])
}

/// The switch's line in `sunrise-extras.txt`, where the engine's Extras kept it before.
const KEY: &str = "autoReconnect";
/// The switch's row, and what it says on hover.
const LABEL: &str = "sunrise.extras.auto_reconnect";
const ABOUT: &str = "sunrise.extras.auto_reconnect.tooltip";

fn on() -> bool {
    with_seat(|seat| seat.on)
}

fn set(on: bool) {
    with_seat(|seat| seat.on = on);
}

/// The switch's line of `sunrise-extras.txt`, for `sunrise_platform::settings::register`.
pub static SETTING: Setting = Setting;

pub struct Setting;

impl Store for Setting {
    fn file(&self) -> &'static str {
        "sunrise-extras.txt"
    }
    fn owns(&self, key: &str) -> bool {
        key == KEY
    }
    fn load(&self, _key: &str, value: &str) {
        match value {
            "true" => set(true),
            "false" => set(false),
            _ => {}
        }
    }
    fn save(&self, line: &mut dyn FnMut(&str, &str)) {
        line(KEY, if on() { "true" } else { "false" });
    }
    fn is_default(&self) -> bool {
        !on()
    }
}

/// The switch's row in the Extras page's Multiplayer group, for
/// `sunrise_platform::pages::register`.
pub static SECTION: Section = Section;

pub struct Section;

impl Page for Section {
    fn id(&self) -> &'static str {
        "sunrise-reconnect.extras"
    }
    fn title(&self) -> &'static str {
        "sunrise.extras.header.multiplayer"
    }
    fn place(&self) -> Place {
        Place::Extras
    }
    fn rows(&self, rows: &mut Vec<Row>) {
        rows.push(Row::toggle(LABEL, on()));
    }
    fn act(&self, row: usize, act: Act) {
        if row == 0 && matches!(act, Act::Press | Act::PressBack) {
            set(!on());
        }
    }
    fn tooltip(&self, _row: usize) -> Option<Label> {
        Some(Label::Key(ABOUT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The only test that touches the statics: off, a lost connection plans nothing; on, the
    /// screen counts down to the same server; leaving forgets it.
    #[test]
    fn the_switch_decides_and_its_line_round_trips() {
        assert!(SETTING.is_default());
        let server = Server::new("example.org", "Tester", false);
        let lost = Ended::new("Server closed", None, By::Server);
        HOOKS.connecting(&server);
        HOOKS.ended(&lost);
        assert_eq!(HOOKS.retry(), None);
        SECTION.act(0, Act::Press);
        let mut lines = Vec::new();
        SETTING.save(&mut |key, value| lines.push(format!("{key}:{value}")));
        assert_eq!(lines, ["autoReconnect:true"]);
        HOOKS.connecting(&server);
        HOOKS.ended(&lost);
        let Some(Retry::Wait(line)) = HOOKS.retry() else {
            panic!("no countdown");
        };
        assert!(line.ends_with("(attempt 1 of 5)"), "{line}");
        HOOKS.left(Left::Disconnected);
        assert_eq!(HOOKS.retry(), None);
        SETTING.load(KEY, "false");
        assert!(SETTING.is_default());
    }
}
