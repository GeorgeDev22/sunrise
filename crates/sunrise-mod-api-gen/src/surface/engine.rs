//! Project SDK staging's source catalogs into the surface without rewriting the tree.
use mod_surface::{EngineContract, EngineItem};
use std::path::Path;

pub(super) fn collect(
    root: &Path,
    wire: &crate::points::WireTypes,
    with_internals: bool,
) -> Result<Vec<EngineItem>, String> {
    let mut items = Vec::new();
    for point in crate::points::points_with(root, wire)? {
        // points.json also carries refusals for diagnostics, but none of these rows admits
        // an injection. Losing a point's last listener place is a removal from this surface.
        if point.api_refusal.is_none() {
            items.push(EngineItem::new(
                format!("point:{}", point.target),
                EngineContract::Point {
                    name: point.name,
                    signature_hash: point.signature_hash,
                },
            ));
        }
    }
    for published in crate::loops::collect(root)? {
        items.push(EngineItem::new(
            format!("loop:{}", published.name),
            EngineContract::Loop {
                entry: published.entry,
                generic: published.generic,
                slot: published.slot,
                side: published.side,
            },
        ));
    }
    if !with_internals {
        return Ok(items);
    }
    for internal in crate::internals::rule_catalog(root)? {
        let mut id = format!("internal:{}:{}", internal.kind, internal.path);
        if !internal.cfg.is_empty() {
            id.push_str(&format!(" [{}]", internal.cfg));
        }
        items.push(EngineItem::new(
            id,
            EngineContract::Internal {
                signature: internal.signature,
                signature_hash: internal.hash,
                shape_only: internal.shape_only,
                opaque_reason: internal.opaque_reason.map(str::to_owned),
            },
        ));
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn exports_points_and_loops_from_source_with_opt_in_internals() {
        let tree = tempfile::tempdir().unwrap();
        let root = tree.path();
        let write = |file: &str, text: &str| {
            let path = root.join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write("crates/sunrise-engine-slots/Cargo.toml", "[dependencies]\n");
        let game = "crates/sunrise-client/src/game.rs";
        let source = r#"
fn tick() {}
#[cfg(unix)] fn helper() {}
struct Game;
impl Game {
    #[sunrise_mod_api_macros::sunrise_loop(name = "things.tick", side = "server")]
    fn tick_things(&mut self) { self.tick_things_with(&mut ()) }
    fn tick_things_with<S: ThingSlot>(&mut self, _slot: &mut S) {}
}
"#;
        write(game, source);
        let wire = Default::default();
        let default = collect(root, &wire, false).unwrap();
        assert!(default.iter().any(|item| {
            item.id == "point:sunrise_client::game::tick"
                && matches!(&item.contract, EngineContract::Point { .. })
        }));
        assert_eq!(
            default
                .iter()
                .find(|item| item.id == "loop:things.tick")
                .map(|item| &item.contract),
            Some(&EngineContract::Loop {
                entry: "sunrise_client::game::Game::tick_things".into(),
                generic: "tick_things_with".into(),
                slot: "ThingSlot".into(),
                side: "server".into(),
            })
        );

        write("crates/sunrise-client/Cargo.toml", "[dependencies]\n");
        write("crates/sunrise-client/src/entry.rs", "");
        write("crates/sunrise-mod-api-gen/internals.txt", "");
        write(crate::crossing::CROSSING_FOREIGN_FILE, "[]");
        let audit = collect(root, &wire, true).unwrap();
        assert_eq!(&audit[..default.len()], default.as_slice());
        assert!(
            audit
                .iter()
                .any(|item| item.id == "internal:fn:sunrise_client::game::helper [unix]")
        );

        write(game, &source.replace("things.tick", "things.step"));
        let surface = |items| {
            mod_surface::Surface::new(
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
                vec![],
            )
            .with_engine_items(items)
        };
        let renamed = surface(collect(root, &wire, false).unwrap());
        let diff = mod_surface::diff(&surface(default), &renamed);
        assert_eq!(
            diff.undeclared()
                .map(|change| change.id.as_str())
                .collect::<Vec<_>>(),
            ["loop:things.tick"]
        );
    }
}
