//! The `screen_click` hook: declarations the mod API generator reads (see `hooks/mod.rs`).
// catalog-order: 0029

pub trait Hooks {
    /// A left or right (`right`) press at `at` (GUI pixels from the screen's
    /// top left, so outside it too) on the screen of mod menu type `menu`
    /// (`block` as in `screen`), over no slot: set `button` to a menu button
    /// id when it is on one of yours.
    /// Sunrise sends it to the server (`ContainerButtonClick`,
    /// `menu_button` there, the block entity's) and the press goes no
    /// further. Only a block's menu (one `block` names) has a `menu_button`
    /// to hear it: a button on a menu opened without one reaches no mod.
    #[hook(
        version = 2,
        side = "client",
        contract = "replace",
        out = "button",
        scope = "menu",
        key = "menu",
        neoforge = "none (see screen)"
    )]
    fn screen_click();
}
