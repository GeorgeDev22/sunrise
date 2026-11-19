//! OptiFine capes (`optifineCapes`, docs/extras.md): a player who got a cape from OptiFine wears
//! it, seen by everyone in Sunrise as OptiFine and the cape mods for vanilla show it, with no
//! OptiFine running.
//!
//! A Sunrise crate (docs/layers.md): while its switch is on it names OptiFine's cape server to
//! the game through the platform's hooks ([`HOOKS`]: `optifine_capes`); the engine checks each
//! player's name, downloads their cape and draws it. The official executable
//! (`sunrise-official`) registers it with its switch: its line of `sunrise-extras.txt`
//! ([`SETTING`]) and its row in Extras' Multiplayer group ([`SECTION`]). Off, the cape server is
//! asked nothing. sunrise-client's own executable never asks it.
#![forbid(unsafe_code)]

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use sunrise_platform::hooks::Hooks;
use sunrise_platform::pages::{Act, Label, Page, Place, Row};
use sunrise_platform::settings::Store;

/// The switch's label and tooltip, registered only with the crate.
pub static LANG: &sunrise_platform::lang::Files =
    &[("en_us", include_str!("../assets/lang/en_us.json"))];

/// Its hook, for `sunrise_platform::hooks::register`.
pub static HOOKS: OptifineCapes = OptifineCapes;

pub struct OptifineCapes;

impl Hooks for OptifineCapes {
    /// The cape server, while the switch is on.
    fn optifine_capes(&self) -> Option<&'static str> {
        on().then(server)
    }
}

/// Where the capes are asked for: OptiFine's server, or the one `SUNRISE_OPTIFINE_CAPES` names
/// (a test's own), read once.
fn server() -> &'static str {
    static SERVER: OnceLock<String> = OnceLock::new();
    SERVER.get_or_init(|| {
        std::env::var("SUNRISE_OPTIFINE_CAPES")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| sunrise_addresses::OPTIFINE_CAPES.to_owned())
    })
}

/// The switch's line in `sunrise-extras.txt`.
const KEY: &str = "optifineCapes";
/// The switch's row, and what it says on hover.
const LABEL: &str = "sunrise.extras.optifine_capes";
const ABOUT: &str = "sunrise.extras.optifine_capes.tooltip";

static ON: AtomicBool = AtomicBool::new(true);

fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

fn set(on: bool) {
    ON.store(on, Ordering::Relaxed);
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
        on()
    }
}

/// The switch's row in the Extras page's Multiplayer group, for
/// `sunrise_platform::pages::register`.
pub static SECTION: Section = Section;

pub struct Section;

impl Page for Section {
    fn id(&self) -> &'static str {
        "sunrise-optifine-capes.extras"
    }
    fn title(&self) -> &'static str {
        "sunrise.extras.header.multiplayer"
    }
    fn place(&self) -> Place {
        Place::Extras
    }
    fn rows(&self, rows: &mut Vec<Row>) {
        rows.push(Row::Toggle {
            label: LABEL,
            on: on(),
        });
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

    /// The only test that touches the switch: on, the game is given the cape server; off, none,
    /// so nothing is asked; its line round-trips, and a value it doesn't know keeps it.
    #[test]
    fn the_switch_decides_whether_the_cape_server_is_named() {
        assert!(SETTING.is_default());
        assert_eq!(HOOKS.optifine_capes(), Some(server()));
        SECTION.act(0, Act::Press);
        assert_eq!(HOOKS.optifine_capes(), None);
        let mut lines = Vec::new();
        SETTING.save(&mut |key, value| lines.push(format!("{key}:{value}")));
        assert_eq!(lines, ["optifineCapes:false"]);
        assert!(!SETTING.is_default());
        SETTING.load(KEY, "maybe");
        assert_eq!(HOOKS.optifine_capes(), None);
        SETTING.load(KEY, "true");
        assert!(SETTING.is_default());
        assert_eq!(HOOKS.optifine_capes(), Some(server()));
    }
}
