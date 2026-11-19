//! `SUNRISE_TICK_SCRIPT=PATH` (tools/test_wire_stream_parity.py): a server's client plays a tick-indexed
//! script, the one the vanilla client plays in `VanillaTickScript.java`, so the two clients' serverbound
//! streams can be compared tick by tick. The harness writes the scenario to PATH (JSON `{"name", "ticks",
//! "steps": {"TICK": [LINE...]}}`, LINE a sunrise-ctl line: `key w down`, `mouse left click`, `look 90 0`,
//! `click-slot 3`, `close`), then creates PATH with the extension `go`. From the next client tick the script
//! plays: its first tick sends `/wire-mark`, which marks it in the capture, then each tick's lines act
//! before the tick runs, as a player's input in the frame before it; when its ticks have run, the client
//! writes the scenario's name to PATH with the extension `done`.
use super::Game;
use super::event_loop::EventLoopCtl;
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
pub(super) struct TickScript {
    /// PATH, when the variable is set.
    path: Option<PathBuf>,
    running: Option<Running>,
}

struct Running {
    name: String,
    ticks: u32,
    steps: BTreeMap<u32, Vec<String>>,
    tick: u32,
}

impl TickScript {
    pub(super) fn from_env() -> Self {
        Self {
            path: std::env::var_os("SUNRISE_TICK_SCRIPT").map(PathBuf::from),
            running: None,
        }
    }
    /// Whether a scenario is playing.
    pub(super) fn playing(&self) -> bool {
        self.running.is_some()
    }
    /// Whether the variable is set (the frame asks before every tick).
    pub(super) fn armed(&self) -> bool {
        self.path.is_some()
    }
}

/// The scenario at `text`: its name, its length in ticks and each tick's lines.
fn parse(text: &str) -> Option<Running> {
    let json: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut steps = BTreeMap::new();
    for (tick, lines) in json.get("steps")?.as_object()? {
        let lines = lines
            .as_array()?
            .iter()
            .filter_map(|l| l.as_str().map(str::to_owned))
            .collect();
        steps.insert(tick.parse().ok()?, lines);
    }
    Some(Running {
        name: json.get("name")?.as_str()?.to_owned(),
        ticks: u32::try_from(json.get("ticks")?.as_u64()?).ok()?,
        steps,
        tick: 0,
    })
}

impl Game<'_> {
    /// Before a client tick: start the scenario the harness handed over, or play this tick's lines.
    pub(super) fn tick_script_step(&mut self, event_loop: &dyn EventLoopCtl) {
        let Some(path) = self.tick_script.path.clone() else {
            return;
        };
        if self.tick_script.running.is_none() {
            let go = path.with_extension("go");
            if self.remote.is_none() || !go.exists() {
                return;
            }
            let _ = std::fs::remove_file(&go);
            let Some(script) = std::fs::read_to_string(&path).ok().and_then(|t| parse(&t)) else {
                log::warn!("Sunrise: tick script {} is not a scenario", path.display());
                return;
            };
            self.tick_script.running = Some(script);
            self.send_signed_chat_input("/wire-mark");
        }
        let Some(running) = self.tick_script.running.as_mut() else {
            return;
        };
        let tick = running.tick;
        running.tick += 1;
        let lines = running.steps.remove(&tick).unwrap_or_default();
        let finished = (running.tick > running.ticks).then(|| running.name.clone());
        // A drag's clicks go out together, as a screen sends them when the button comes up
        // (AbstractContainerScreen.mouseReleased): the menu predicts them as one quick craft.
        let mut drag = Vec::new();
        for line in lines {
            match self.tick_script_click(&line) {
                Some(Some(op)) if op.2 == sunrise_client::menu::Input::QuickCraft => drag.push(op),
                Some(op) => {
                    self.flush_tick_script_drag(&mut drag);
                    if let Some(op) = op {
                        self.remote_menu_ops(&[op]);
                    }
                }
                None => {
                    self.flush_tick_script_drag(&mut drag);
                    if !self.tick_script_menu_line(&line) {
                        self.replay_line(&line, event_loop);
                    }
                }
            }
        }
        self.flush_tick_script_drag(&mut drag);
        if let Some(name) = finished {
            self.tick_script.running = None;
            let _ = std::fs::write(path.with_extension("done"), name);
        }
    }
}

/// A click on the open menu: Sunrise's slot (None outside the screen), its button and input.
type MenuOp = (Option<usize>, i32, sunrise_client::menu::Input);

impl Game<'_> {
    /// The menu lines, without a renderer, as the vanilla driver takes them. sunrise-ctl's
    /// versions press where the screen draws them, and `--headless` draws nothing; the vanilla
    /// driver works the menu itself (`MultiPlayerGameMode.handleContainerInput`), and so does this.
    /// - `menu-button N`: a station screen's button (a stonecutter's recipe, a loom's pattern, an
    ///   enchantment), `clickMenuButton` and then `handleInventoryButtonClick`;
    /// - `select-trade N`: a merchant screen's offer (`MerchantScreen.postButtonClick`).
    fn tick_script_menu_line(&mut self, line: &str) -> bool {
        let words: Vec<&str> = line.split_whitespace().collect();
        let number = words.get(1).and_then(|w| w.parse::<i32>().ok());
        match (words.first(), number) {
            (Some(&"menu-button"), Some(button)) => self.station_menu_button(button),
            (Some(&"select-trade"), Some(offer)) => {
                if let Ok(offer) = usize::try_from(offer) {
                    self.try_move_items(offer);
                }
            }
            _ => return false,
        }
        true
    }
    /// `click-slot SLOT [right] [shift] [INPUT] [bBUTTON]`: SLOT is the open menu's own slot
    /// (-999 outside the screen), INPUT a `ContainerInput` name and BUTTON its button. None for
    /// another line; Some(None) for a slot the open menu doesn't have.
    fn tick_script_click(&self, line: &str) -> Option<Option<MenuOp>> {
        use sunrise_client::menu::Input;
        let words: Vec<&str> = line.split_whitespace().collect();
        let (Some(&"click-slot"), Some(Ok(slot))) =
            (words.first(), words.get(1).map(|w| w.parse::<i32>()))
        else {
            return None;
        };
        let rest = &words[2..];
        let mut input = if rest.contains(&"shift") {
            Input::QuickMove
        } else {
            Input::Pickup
        };
        let mut button = i32::from(rest.contains(&"right"));
        for word in rest {
            if let Some(named) = Input::parse(word) {
                input = named;
            }
            if let Some(Ok(b)) = word.strip_prefix('b').map(str::parse) {
                button = b;
            }
        }
        Some(self.remote_menu_slot_op(slot, button, input))
    }
    fn flush_tick_script_drag(&mut self, drag: &mut Vec<MenuOp>) {
        if !drag.is_empty() {
            self.remote_menu_ops(drag);
            drag.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scenario_names_its_ticks_and_their_lines() {
        let s = parse(r#"{"name":"walk","ticks":40,"steps":{"0":["key w down"],"20":["key w up","look 90 0"]}}"#)
            .unwrap();
        assert_eq!((s.name.as_str(), s.ticks), ("walk", 40));
        assert_eq!(s.steps[&20], ["key w up", "look 90 0"]);
        assert!(parse(r#"{"name":"x","ticks":1,"steps":{"a":[]}}"#).is_none());
    }
}
