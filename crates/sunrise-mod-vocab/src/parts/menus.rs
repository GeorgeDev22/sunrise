use super::*;

// no wire: frozen registration storage; each boundary value has its own Wire derive.
#[derive(Clone)]
pub(crate) struct MenuBookDeclaration {
    pub(crate) owner: &'static str,
    pub(crate) book: crate::RecipeBookTypeKey,
    pub(crate) recipe_type: String,
    pub(crate) layout: crate::RecipeBookLayout,
    pub(crate) screen: crate::Screen,
}

impl HookParts {
    pub(crate) fn menu<'k>(
        &self,
        id: impl FnOnce() -> Option<&'k str>,
    ) -> &[Attached<dyn MenuPart>] {
        attached(&self.menus, id)
    }
    /// The menu's owner lookup: only the part whose package owns the menu type, or the block
    /// opening a vanilla menu, receives callbacks. A registered recipe-book screen is owned by
    /// its declaring mod even though its menu type is vanilla.
    pub(crate) fn menu_screen(&self, menu: &str, block: Option<&str>) -> &[Attached<dyn MenuPart>] {
        let parts = attached(&self.menus, || Some(menu));
        let owner = crate::RecipeBookLayout::declaring_mod(menu);
        let namespace = match owner {
            Some(owner) => owner,
            None if vanilla_menu(menu) => {
                let Some((namespace, _)) = block.and_then(|b| b.split_once(':')) else {
                    return &[];
                };
                namespace
            }
            None => {
                let Some((namespace, _)) = menu.split_once(':') else {
                    return &[];
                };
                namespace
            }
        };
        match parts
            .iter()
            .position(|p| owns_namespace(p.mod_id, namespace))
        {
            Some(i) => std::slice::from_ref(&parts[i]),
            None => &[],
        }
    }
}

/// The hooks a menu type's screen is made of, one part per screen answering them all.
const SCREEN_HOOKS: [HookId; 4] = [
    HookId::Screen,
    HookId::ScreenDraw,
    HookId::ScreenClick,
    HookId::ScreenTooltip,
];

/// Whether `part` answers a screen hook: then it owns the screens it is asked for.
pub fn lays_out_screen(part: &dyn MenuPart) -> bool {
    let mut subscribed = part.menu_subscribed().iter().flatten();
    subscribed.any(|&hook| {
        SCREEN_HOOKS
            .iter()
            .any(|&screen| screen as usize == hook as usize)
    })
}

/// Whether screen parts of mods `a` and `b` on menu type `menu` would own the same screens,
/// which registration refuses: always on a mod's menu type, and on one of vanilla's only for
/// one mod, as each mod's blocks get their own mod's part (`HookParts::menu_screen`).
pub fn screens_shared(menu: &str, a: &str, b: &str) -> bool {
    !vanilla_menu(menu) || a == b
}

fn vanilla_menu(menu: &str) -> bool {
    menu.starts_with("minecraft:")
}

/// The mod that declared the recipe-book screen or owns the dynamic screen of `menu`
/// opened as `block`'s menu. A registered book needs no screen part to have an owner.
pub fn menu_screen_owner(menu: &str, block: Option<&str>) -> Option<&'static str> {
    if let Some(owner) = crate::RecipeBookLayout::declaring_mod(menu) {
        return Some(owner);
    }
    SCREEN_HOOKS
        .into_iter()
        .filter(|&hook| has_part_hook(hook))
        .find_map(|hook| {
            let parts = parts(hook, "screen owner");
            parts
                .for_hook(hook)?
                .menu_screen(menu, block)
                .first()
                .map(|p| p.mod_id)
        })
}

/// Whether the Parts attached to the menu type `menu` subscribe to `hook`; the game
/// asks once when a menu opens, so a menu with none skips the hook while it is open.
pub fn menu_listens(hook: HookId, menu: &str) -> bool {
    if !has_part_hook(hook) {
        return false;
    }
    parts(hook, "menu routing")
        .for_hook(hook)
        .is_some_and(|table| !table.menu(|| Some(menu)).is_empty())
}

impl dyn Registrar + '_ {
    /// Attach `part` to the menu type `menu`: a mod's, or one of vanilla's that a mod block opens.
    pub fn menu_part<P: MenuPart>(&mut self, menu: &str, part: P) {
        self.attach_menu_part(menu, type_name::<P>(), Arc::new(part));
    }
}

/// Registrar operations for menus.
pub trait MenusRegistrar {
    /// Declare this menu's book, authoritative recipe type and destination roles.
    /// The menu remains content-owned; loading checks these roles against its
    /// actual container slots and crafting declaration. `screen` is the existing
    /// typed screen value, frozen here: mapped slots need finite coordinates and
    /// its book must equal `book`. This screen owns client geometry and server
    /// hidden-slot policy; these menus do not call the dynamic screen hook.
    /// The declaring mod owns the screen's callbacks and slot controls, even
    /// without a screen part. A foreign screen part on this menu is refused.
    /// This does not turn display candidates into ingredient predicates.
    /// Invalid ownership, unknown books, duplicate menus or malformed screen
    /// geometry refuse installation, even when the caller ignores the error.
    fn menu_recipe_book(
        &mut self,
        menu: &str,
        book: &crate::RecipeBookTypeKey,
        recipe_type: &str,
        layout: crate::RecipeBookLayout,
        screen: crate::Screen,
    ) -> Result<(), String>;
    /// Attach `part`, of kind `kind`, to the menu type `menu`.
    fn attach_menu_part(&mut self, menu: &str, kind: &'static str, part: Arc<dyn MenuPart>);
}
