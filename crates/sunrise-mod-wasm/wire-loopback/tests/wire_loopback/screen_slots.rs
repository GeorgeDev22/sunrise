//! `ScreensClientHost::screen_slot` and `set_screen_slot` from a wasm mod reach the client
//! with the menu type, the slot and the position intact, and each named refusal comes back as the
//! native call returns it.
use super::*;
use mod_api::ScreenSlotRefusal;
use mod_api::prelude::*;
use std::cell::RefCell;

/// What reached the client: the menu type, the slot and the position asked for.
type Call = (String, usize, Option<[f32; 2]>);

/// A client with one open screen of `machines:station` and three slots, the third hidden at
/// open, that answers as the game would and records what reached it.
#[derive(Default)]
struct Client {
    calls: RefCell<Vec<Call>>,
}
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl mod_api::ClientHost for Client {}
mod_api::impl_client_host_areas!(Client, except(screens));
impl ScreensClientHost for Client {
    fn screen_slot(&self, menu: &str, slot: usize) -> Option<[f32; 2]> {
        (menu == "machines:station" && slot < 2).then_some([slot as f32 * 18.0, 17.0])
    }
    fn set_screen_slot(
        &self,
        menu: &str,
        slot: usize,
        pos: Option<[f32; 2]>,
    ) -> Result<(), ScreenSlotRefusal> {
        self.calls.borrow_mut().push((menu.to_owned(), slot, pos));
        match (menu, slot, pos) {
            (b, ..) if b != "machines:station" => Err(ScreenSlotRefusal::NoScreen),
            (_, 3.., _) => Err(ScreenSlotRefusal::NoSuchSlot),
            (_, 2, Some(_)) => Err(ScreenSlotRefusal::HiddenAtOpen),
            (_, _, Some([x, _])) if !x.is_finite() => Err(ScreenSlotRefusal::BadPosition),
            _ => Ok(()),
        }
    }
}

#[test]
fn a_wasm_mods_slot_moves_reach_the_client_and_get_each_refusal_by_name() {
    mod_api::__private::initialize().unwrap();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let mut client = Client::default();
    let mut frame = boundary::Frame::default();
    frame.set_client_host_mut(&mut client);
    let mut cx = boundary::Cx::default();
    cx.frames.push(frame);
    let cx = RefCell::new(cx);
    guest::set_transport(Some(Rc::new(move |id, bytes| {
        let mut out = Writer::new();
        // SAFETY: client and its frame remain live for this synchronous transport.
        unsafe { boundary::serve(id, &mut *cx.borrow_mut(), bytes, &mut out) }.unwrap();
        out.into_bytes()
    })));
    let asks: [(&str, usize, Option<[f32; 2]>, Result<(), ScreenSlotRefusal>); 6] = [
        ("machines:station", 0, Some([30.0, 40.5]), Ok(())),
        ("machines:station", 1, None, Ok(())),
        (
            "machines:station",
            2,
            Some([0.0, 0.0]),
            Err(ScreenSlotRefusal::HiddenAtOpen),
        ),
        (
            "machines:station",
            3,
            None,
            Err(ScreenSlotRefusal::NoSuchSlot),
        ),
        (
            "machines:station",
            0,
            Some([f32::INFINITY, 0.0]),
            Err(ScreenSlotRefusal::BadPosition),
        ),
        ("other:menu", 0, None, Err(ScreenSlotRefusal::NoScreen)),
    ];
    for (menu, slot, pos, answer) in asks {
        assert_eq!(guest::Guest.set_screen_slot(menu, slot, pos), answer);
    }
    assert_eq!(
        guest::Guest.screen_slot("machines:station", 1),
        Some([18.0, 17.0])
    );
    assert_eq!(guest::Guest.screen_slot("machines:station", 2), None);
    guest::set_transport(None);
    let reached: Vec<Call> = asks
        .iter()
        .map(|(b, s, p, _)| (b.to_string(), *s, *p))
        .collect();
    assert_eq!(*client.calls.borrow(), reached);
}

#[test]
fn every_screen_slot_refusal_round_trips_on_the_wire() {
    for refusal in [
        ScreenSlotRefusal::NoScreen,
        ScreenSlotRefusal::NoSuchSlot,
        ScreenSlotRefusal::HiddenAtOpen,
        ScreenSlotRefusal::BadPosition,
        ScreenSlotRefusal::Unavailable,
    ] {
        let mut w = Writer::new();
        w.put(&Err::<(), _>(refusal));
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        let back: Result<(), ScreenSlotRefusal> = r.take().unwrap();
        assert_eq!(back, Err(refusal));
    }
}
