use super::*;
fn screen(book: &crate::RecipeBookTypeKey) -> crate::Screen {
    crate::Screen {
        width: 176.0,
        height: 166.0,
        slots: vec![
            Some(crate::ScreenSlot {
                position: [8.0, 8.0],
                ..Default::default()
            });
            3
        ],
        recipe_book: Some(book.clone()),
        ..Default::default()
    }
}
fn roles() -> crate::RecipeBookLayout {
    crate::RecipeBookLayout::Furnace {
        input: 0,
        fuel: 1,
        result: 2,
    }
}
#[test]
fn registration_refuses_unknown_books_duplicate_menus_and_overlapping_roles() {
    let mut registrar = PartRegistrar::new("demo");
    let book = registrar.recipe_book_type("demo:book").unwrap();
    assert!(
        registrar
            .menu_recipe_book("demo:pot", &book, "demo:cooking", roles(), screen(&book))
            .is_ok()
    );
    assert!(
        registrar
            .menu_recipe_book("demo:pot", &book, "demo:cooking", roles(), screen(&book))
            .is_err()
    );
    assert!(
        registrar
            .menu_recipe_book("other:pot", &book, "demo:cooking", roles(), screen(&book))
            .is_err()
    );
    assert!(
        registrar
            .menu_recipe_book(
                "demo:bad",
                &book,
                "demo:cooking",
                crate::RecipeBookLayout::Stonecutter {
                    input: 0,
                    result: 0
                },
                screen(&book)
            )
            .is_err()
    );
    let unknown = mod_vocab::__private::parts_installation::recipe_book_type("demo:unknown".into());
    assert!(
        registrar
            .menu_recipe_book(
                "demo:unknown",
                &unknown,
                "demo:cooking",
                roles(),
                screen(&book)
            )
            .is_err()
    );
    assert_eq!(registrar.menu_recipe_books.len(), 1);
    assert_eq!(registrar.refused.len(), 4);
    assert_eq!(
        registrar.menu_recipe_books["demo:pot"].screen,
        screen(&book)
    );
}
struct Overlay;
#[mod_api::sunrise_part]
impl MenuPart for Overlay {
    fn screen_draw(&self, _: &dyn ClientHost, _: &mut ScreenDrawEvent<'_>) {}
}

#[test]
fn a_foreign_screen_part_conflicts_with_a_registered_book_in_either_order() {
    let menu = "demo:pot";
    for book_first in [true, false] {
        let mut registrar = PartRegistrar::new("demo");
        let book = registrar.recipe_book_type("demo:book").unwrap();
        let attach = |registrar: &mut PartRegistrar| {
            registrar.mod_id = "rival";
            (registrar as &mut dyn Registrar).menu_part(menu, Overlay);
            registrar.mod_id = "demo";
        };
        if !book_first {
            attach(&mut registrar);
        }
        let registered =
            registrar.menu_recipe_book(menu, &book, "demo:cooking", roles(), screen(&book));
        if book_first {
            assert!(registered.is_ok());
            attach(&mut registrar);
            assert!(!registrar.parts.menus.contains_key(menu));
        } else {
            assert!(registered.is_err());
            assert!(!registrar.menu_recipe_books.contains_key(menu));
        }
        let [refusal] = registrar.refused.as_slice() else {
            panic!("expected one conflict, got {:?}", registrar.refused);
        };
        for name in [menu, "mod demo", "mod rival"] {
            assert!(refusal.contains(name), "{refusal} does not name {name}");
        }
    }
}

#[test]
fn missing_nonfinite_and_wrong_book_geometry_refuse_registration() {
    let mut registrar = PartRegistrar::new("demo");
    let book = registrar.recipe_book_type("demo:book").unwrap();
    let mut bad = screen(&book);
    bad.slots[1] = None;
    let error = registrar
        .menu_recipe_book("demo:missing", &book, "demo:cooking", roles(), bad)
        .unwrap_err();
    assert!(error.contains("mod demo menu \"demo:missing\"") && error.contains("slot 1"));
    let mut bad = screen(&book);
    bad.slots[1] = Some(crate::ScreenSlot {
        position: [f32::NAN, 0.0],
        ..Default::default()
    });
    assert!(
        registrar
            .menu_recipe_book("demo:nan", &book, "demo:cooking", roles(), bad)
            .unwrap_err()
            .contains("finite")
    );
    let mut bad = screen(&book);
    bad.recipe_book = None;
    assert!(
        registrar
            .menu_recipe_book("demo:wrong", &book, "demo:cooking", roles(), bad)
            .unwrap_err()
            .contains("screen book differs")
    );
    assert!(registrar.menu_recipe_books.is_empty());
    assert_eq!(registrar.refused.len(), 3);
}
