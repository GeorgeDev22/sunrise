//! `panel_search` from a guest: the host answers it inside the calling mod's own scope, the
//! identity its owner check reads (`crash::current_native_mod`), whatever id the guest names.
use super::*;
use mod_api::content::crash::{ModCall, current_native_mod, enter_native_mod};
use mod_api::prelude::RecipesClientHost;
use mod_api::{PanelStyle, RecipeTransferCheck, Stack};
use std::sync::Mutex;

/// Each call the host answered: the running mod, the id named and the text.
static CALLS: Mutex<Vec<(Option<&'static str>, String, String)>> = Mutex::new(Vec::new());

struct Client;
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl mod_api::ClientHost for Client {}
mod_api::impl_client_host_areas!(Client, except(recipes));
// A boundary fixture: the rest of the area is not called.
impl RecipesClientHost for Client {
    fn panel_items(&mut self, _: &str, _: Vec<Stack>) {
        unreachable!()
    }
    fn panel_style(&mut self, _: &str, _: PanelStyle) {
        unreachable!()
    }
    fn panel_search(&mut self, mod_id: &str, text: &str) {
        let call = (current_native_mod(), mod_id.to_owned(), text.to_owned());
        CALLS.lock().unwrap().push(call);
    }
    fn place_recipe(&mut self, _: i32, _: bool) -> Result<(), String> {
        unreachable!()
    }
    fn transfer_recipe(&mut self, _: i32) {
        unreachable!()
    }
    fn transfer_recipe_id(&mut self, _: &str, _: bool) {
        unreachable!()
    }
    fn transfer_recipe_check(&mut self, _: &str) -> Option<RecipeTransferCheck> {
        unreachable!()
    }
    fn recipe_book_ids(&mut self, _: &str) -> Arc<[i32]> {
        unreachable!()
    }
}

#[test]
fn a_guests_panel_search_is_answered_as_its_own_mod() {
    install_counter();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let mut client = Client;
    let mut frame = boundary::Frame::default();
    frame.set_client_host_mut(&mut client);
    let mut cx = boundary::Cx::default();
    cx.frames.push(frame);
    let cx = std::cell::RefCell::new(cx);
    guest::set_transport(Some(Rc::new(move |id, bytes| {
        let mut out = Writer::new();
        // SAFETY: the client and its frame outlive this synchronous transport.
        unsafe { boundary::serve(id, &mut *cx.borrow_mut(), bytes, &mut out) }.unwrap();
        out.into_bytes()
    })));
    for (caller, text) in [("jei", "iron"), ("ae2", "gold")] {
        let _call = enter_native_mod(ModCall {
            id: caller,
            hook: "panel_event",
            part: None,
        });
        guest::Guest.panel_search("jei", text);
    }
    guest::set_transport(None);
    assert_eq!(
        std::mem::take(&mut *CALLS.lock().unwrap()),
        [
            (Some("jei"), "jei".to_owned(), "iron".to_owned()),
            (Some("ae2"), "jei".to_owned(), "gold".to_owned()),
        ]
    );
}
