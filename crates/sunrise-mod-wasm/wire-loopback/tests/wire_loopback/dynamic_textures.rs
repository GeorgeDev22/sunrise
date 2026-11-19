//! `ClientRenderClientHost::create_dynamic_texture`, `update_dynamic_texture`,
//! `free_dynamic_texture` and `dynamic_texture_budget` from a wasm mod reach the client with the
//! id, the size and the pixels intact, and each named refusal comes back as the native call
//! returns it (#1343).
use super::*;
use mod_api::prelude::*;
use mod_api::{DynamicTextureBudget, DynamicTextureError, Identifier};
use std::cell::RefCell;

/// A client that refuses as the game would for these ids and sizes, and records what reached
/// it.
#[derive(Default)]
struct Client {
    calls: RefCell<Vec<(&'static str, String, u32, u32, Vec<u8>)>>,
}
impl mod_api::Host for Client {}
mod_api::impl_host_areas!(Client);
impl mod_api::ClientHost for Client {}
mod_api::impl_client_host_areas!(Client, except(client_render));
/// The refusal the client gives an image of `width` pixels: each one a width of its own.
fn refusal(id: &Identifier, width: u32) -> Result<(), DynamicTextureError> {
    use DynamicTextureError::*;
    match (id.namespace(), width) {
        ("other", _) => Err(NotOwned),
        (_, 0) => Err(BadSize),
        (_, 2048) => Err(TooLarge),
        (_, 1000) => Err(NoSlot),
        (_, 999) => Err(OverBudget),
        (_, 998) => Err(NameTaken),
        (_, 997) => Err(Unknown),
        (_, 996) => Err(ClassChange),
        (_, 995) => Err(Unavailable),
        _ => Ok(()),
    }
}
impl ClientRenderClientHost for Client {
    fn create_dynamic_texture(
        &mut self,
        id: &Identifier,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError> {
        let call = ("create", id.to_string(), width, height, rgba.to_vec());
        self.calls.borrow_mut().push(call);
        refusal(id, width)
    }
    fn update_dynamic_texture(
        &mut self,
        id: &Identifier,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), DynamicTextureError> {
        let call = ("update", id.to_string(), width, height, rgba.to_vec());
        self.calls.borrow_mut().push(call);
        refusal(id, width)
    }
    fn free_dynamic_texture(&mut self, id: &Identifier) -> Result<(), DynamicTextureError> {
        let call = ("free", id.to_string(), 0, 0, Vec::new());
        self.calls.borrow_mut().push(call);
        match id.path() {
            "panel" => Ok(()),
            _ => Err(DynamicTextureError::Unknown),
        }
    }
    fn dynamic_texture_budget(&self) -> Result<DynamicTextureBudget, DynamicTextureError> {
        Ok(mod_api::__private::built::dynamic_texture_budget(
            15,
            (4 << 20) - 24,
        ))
    }
}

#[test]
fn a_wasm_mods_texture_reaches_the_client_and_gets_each_refusal_by_name() {
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
    let id = |text: &str| Identifier::parse(text).unwrap();
    let pixels: Vec<u8> = (0..2 * 3 * 4).collect();
    let mut guest = guest::Guest;
    assert_eq!(
        guest.create_dynamic_texture(&id("mymod:panel"), 2, 3, &pixels),
        Ok(())
    );
    assert_eq!(
        guest.dynamic_texture_budget(),
        Ok(mod_api::__private::built::dynamic_texture_budget(
            15,
            (4 << 20) - 24
        ))
    );
    let refused = [
        ("other:panel", 2, DynamicTextureError::NotOwned),
        ("mymod:panel", 0, DynamicTextureError::BadSize),
        ("mymod:panel", 2048, DynamicTextureError::TooLarge),
        ("mymod:panel", 1000, DynamicTextureError::NoSlot),
        ("mymod:panel", 999, DynamicTextureError::OverBudget),
        ("mymod:panel", 998, DynamicTextureError::NameTaken),
        ("mymod:panel", 997, DynamicTextureError::Unknown),
        ("mymod:panel", 996, DynamicTextureError::ClassChange),
        ("mymod:panel", 995, DynamicTextureError::Unavailable),
    ];
    for (texture, width, refusal) in refused {
        let answer = guest.create_dynamic_texture(&id(texture), width, 1, &[1, 2, 3, 4]);
        assert_eq!(answer, Err(refusal), "create {texture}, {width} wide");
        let answer = guest.update_dynamic_texture(&id(texture), width, 1, &[5, 6, 7, 8]);
        assert_eq!(answer, Err(refusal), "update {texture}, {width} wide");
    }
    assert_eq!(
        guest.update_dynamic_texture(&id("mymod:panel"), 3, 2, &pixels),
        Ok(())
    );
    assert_eq!(guest.free_dynamic_texture(&id("mymod:panel")), Ok(()));
    assert_eq!(
        guest.free_dynamic_texture(&id("mymod:gone")),
        Err(DynamicTextureError::Unknown)
    );
    guest::set_transport(None);
    let mut reached = vec![("create", "mymod:panel".to_string(), 2, 3, pixels.clone())];
    for (texture, width, _) in refused {
        reached.push(("create", texture.into(), width, 1, vec![1, 2, 3, 4]));
        reached.push(("update", texture.into(), width, 1, vec![5, 6, 7, 8]));
    }
    reached.push(("update", "mymod:panel".into(), 3, 2, pixels));
    reached.push(("free", "mymod:panel".into(), 0, 0, Vec::new()));
    reached.push(("free", "mymod:gone".into(), 0, 0, Vec::new()));
    assert_eq!(*client.calls.borrow(), reached);
}
