//! #792 stage 4b: the entity part's wasm crossing. The counter mirrors the
//! native `tick_tests` fixture — its count lives only in the row's
//! attachments (#796), never in the part — and traps on request, after its
//! write, so the game's containment rolls the row back.
use mod_api::{
    AttachmentCodec, AttachmentError, AttachmentType, EntityPart, GeneralEntityCategory,
    GeneralEntityTickEvent, GeneralSavePolicy, Holder, Registrar, ServerHost,
};
use mod_api::{Compound, Tag};

/// The row's counter, as the native fixture's: a codec'd attachment, so the
/// count is per-row even though the part is one shared object.
pub static COUNTER: AttachmentType<i32> = AttachmentType {
    namespace: "wasm_fixture",
    name: "entity_counter",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
        decode: |tag: &Compound| match tag.get(&"n".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// When nonzero, the row's tick traps once its count reaches this value,
/// after the write: the host's transaction must roll the row back to the
/// count below it. A test arms it through the saved form, the way a saved
/// world would: the codec keeps it readable after a load.
pub static TRAP_AFTER: AttachmentType<i32> = AttachmentType {
    namespace: "wasm_fixture",
    name: "entity_trap_after",
    default: || 0,
    codec: Some(AttachmentCodec {
        encode: |n| Ok([("n".into(), Tag::Int(*n))].into()),
        decode: |tag: &Compound| match tag.get(&"n".into()) {
            Some(Tag::Int(n)) => Ok(*n),
            _ => Err(AttachmentError::Decode),
        },
    }),
    copy_on_death: false,
    sync: false,
};

/// `wasm_fixture:robot`'s part: the acceptance matrix's counter, on wasm.
pub struct Counter;

#[mod_api::sunrise_part]
impl EntityPart for Counter {
    fn general_entity_tick(
        &self,
        host: &mut dyn ServerHost,
        event: &mut GeneralEntityTickEvent<'_>,
    ) {
        let holder = Holder::Entity(event.entity.id());
        let value = COUNTER.get(host, holder).unwrap() + 1;
        COUNTER.set(host, holder, value).unwrap();
        let trap_after = TRAP_AFTER.get(host, holder).unwrap();
        if trap_after != 0 && value >= trap_after {
            panic!("general entity tick wasm boom");
        }
    }
}

/// Declares the fixture's general entity type and attaches the counter part.
pub fn register(registrar: &mut dyn Registrar) {
    // The attachment types the part reads and writes: a package uses only the types it declared.
    for name in ["entity_counter", "entity_trap_after"] {
        registrar
            .register_attachment_type(&format!("wasm_fixture:{name}"), false)
            .unwrap();
    }
    registrar
        .declare_general_entity(
            "robot",
            GeneralEntityCategory::Misc,
            0.8,
            1.9,
            GeneralSavePolicy::Save,
            true,
            8,
            3,
            1,
            &[],
        )
        .unwrap();
    registrar.entity_part("wasm_fixture:robot", Counter);
}
