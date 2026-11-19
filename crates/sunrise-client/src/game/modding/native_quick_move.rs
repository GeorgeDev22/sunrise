//! A shift-click in a mod container's menu, routed by the mods
//! (`native_menu_quick_move`, `AbstractContainerMenu.quickMoveStack`) before
//! the game's own routing.
use super::{Game, decided_action::Epoch};
use crate::game::transactions::decision::{InvalidReason, Reply};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use sunrise_client::{
    block_entities::Pos,
    containers::{BlockEntity, ModBlockEntity, ModMenuType},
    items::ItemStack,
    lan::menu::Slots,
    menu::Asked,
};
use sunrise_protocol::packets::play;

/// `ClickType.QUICK_MOVE`'s id in `ServerboundContainerClickPacket`.
const QUICK_MOVE: i32 = 1;

/// Where a menu slot of a block container's menu lives: the container's own
/// slot `k`, or the player's inventory index (hotbar 0 to 8, then the 27
/// main slots), the menu listing the main slots before the hotbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MenuSlot {
    Container(usize),
    Inventory(usize),
}

/// The slot a quick move of `slot` on a menu with `size` container slots
/// asks a mod about, or None when the click is not one: another click type,
/// a button other than left or right (`doClick` takes 0 and 1), no slot,
/// a slot past the menu, or a container slot the mod's screen hides (the
/// menu has no such slot, so the click goes nowhere).
pub(super) fn quick_move_slot(
    input: i32,
    button: i32,
    slot: i32,
    size: usize,
    hidden: &[usize],
) -> Option<MenuSlot> {
    if input != QUICK_MOVE || !(0..=1).contains(&button) {
        return None;
    }
    let slot = usize::try_from(slot).ok()?;
    if slot < size {
        return (!hidden.contains(&slot)).then_some(MenuSlot::Container(slot));
    }
    let n = slot - size;
    match n {
        0..27 => Some(MenuSlot::Inventory(9 + n)),
        27..36 => Some(MenuSlot::Inventory(n - 27)),
        _ => None,
    }
}

/// What a quick move the mods were asked about leaves: when one took it
/// (`handled`), the clicked slot holds what is left of `stack`, whatever the
/// mods wrote there, and their other changes to the block's items stay;
/// otherwise the block's items are as they were before the call (`before`),
/// so nothing a mod moved without taking the click is kept twice.
fn settle(
    at: MenuSlot,
    handled: bool,
    stack: Option<ItemStack>,
    before: &[Option<ItemStack>],
    items: &mut Vec<Option<ItemStack>>,
    inventory: &mut [Option<ItemStack>],
) {
    if !handled {
        before.clone_into(items);
        return;
    }
    let left = stack;
    match at {
        MenuSlot::Container(k) => {
            if items.len() <= k {
                items.resize(k + 1, None);
            }
            items[k] = left;
        }
        MenuSlot::Inventory(i) => {
            if let Some(s) = inventory.get_mut(i) {
                *s = left;
            }
        }
    }
}

impl Game<'_> {
    /// `ServerGamePacketListenerImpl.handleContainerClick` for a quick move
    /// in a guest's open mod container menu (on a vanilla menu type or the
    /// mod's own): the mods' `native_menu_quick_move` first, as the clicking
    /// player. True when a mod took it: the slot keeps what the mod left and
    /// the guest's client gets the whole menu again. False for every other
    /// click, and for one no mod took, so the game's own routing runs.
    pub(super) fn lan_mod_quick_move(&mut self, conn: u32, p: &play::ContainerClick) -> bool {
        let Some(host) = self.lan.as_ref() else {
            return false;
        };
        let Some(g) = host.hub.guest(conn) else {
            return false;
        };
        let Some(menu) = g
            .menu
            .as_ref()
            .filter(|m| p.container != 0 && m.window == p.container)
        else {
            return false;
        };
        let Slots::Block(pos) = menu.slots else {
            return false;
        };
        let Some(at) = quick_move_slot(
            p.input,
            i32::from(p.button),
            i32::from(p.slot),
            menu.size,
            &menu.hidden,
        ) else {
            return false;
        };
        // A spectator's or a dead player's click and a menu no longer valid
        // change nothing, and a shift-click during a drag only ends the drag
        // (`doClick`'s resetQuickCraft): the game's own path answers them.
        if g.spectator()
            || g.dead
            || menu.drag.is_some()
            || !menu.still_valid(&host.hub.loop_context, &self.world, g)
        {
            return false;
        }
        let (size, window, menu_type) = (menu.size, menu.window, menu.mod_type);
        // A mod menu type keeps its drag in the connection's machine menu
        // (`lan_machine_click`), not in `menu.drag`: a shift-click during it
        // goes there too, to only end the drag.
        if host
            .menus
            .get(&conn)
            .and_then(|play| play.crafting.machine.as_ref())
            .is_some_and(|(w, m)| *w == window && m.quick_crafting())
        {
            return false;
        }
        if !matches!(
            self.world.block_entities.get(&pos),
            Some(BlockEntity::Mod(m)) if !m.id().is_empty()
        ) {
            return false;
        }
        let slot = p.slot as usize;
        // The game's own routing reuses the pickup rule's answer asked before the hook, so the
        // click asks it once, as the client's prediction does.
        let (handled, asked) = if mod_api::__private::has_part_hook(mod_api::HookId::MenuQuickMove)
        {
            // A listener: the move is one action, decided (`decided_action`): the hook's answer
            // is validated and applied once, and a refused move changes nothing.
            match self.decided_mod_quick_move(conn, pos, size, menu_type, slot, at, window) {
                Some(decided) => decided,
                None => {
                    // The client predicted the game's routing: it gets the menu as it is.
                    if let Some(host) = self.lan.as_mut() {
                        host.hub.resend_menu(conn, window, &self.world);
                    }
                    return true;
                }
            }
        } else {
            let handled = self
                .as_guest(conn, |game| game.mod_menu_quick_move(pos, size, slot, at))
                .unwrap_or(false);
            (handled, Asked::default())
        };
        if !handled {
            return !asked.is_empty() && self.lan_machine_click_asked(conn, p, asked);
        }
        // The mod menu type's engine starts over from the block entity next click.
        let Some(host) = self.lan.as_mut() else {
            return false;
        };
        if let Some(play) = host.menus.get_mut(&conn)
            && play
                .crafting
                .machine
                .as_ref()
                .is_some_and(|(w, _)| *w == window)
        {
            *play.crafting.machine = None;
        }
        // sendAllDataToRemote: the client predicted the game's routing. A mod
        // that removed its own block leaves no menu to send: it closes, and
        // the inventory goes out on its own.
        if matches!(
            self.world.block_entities.get(&pos),
            Some(BlockEntity::Mod(_))
        ) {
            host.hub.resend_menu(conn, window, &self.world);
        } else if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
            host.hub.close_container(conn);
            host.hub.send_inventory(gi);
        }
        true
    }

    /// The mods' `native_menu_quick_move` on menu slot `slot` (living at
    /// `at`) of the mod container at `pos` (`size` container slots), for the
    /// player in place: true when
    /// one took it, the slot then holding what is left of its stack. The
    /// block entity is out of the world during the call. This is the call with no part to
    /// answer (it asks nobody, the slot rules included); with a part the move is decided
    /// (`decided_mod_quick_move`).
    fn mod_menu_quick_move(&mut self, pos: Pos, size: usize, slot: usize, at: MenuSlot) -> bool {
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos) else {
            return false;
        };
        let held = match at {
            MenuSlot::Container(k) => m.items.get(k).cloned().flatten(),
            MenuSlot::Inventory(i) => self.inventory.get(i).cloned().flatten(),
        };
        let Some(stack) = held.filter(|s| s.count > 0) else {
            return false;
        };
        let mut stack = Some(stack);
        let mut taken = std::mem::take(&mut **m);
        let items = taken.items.clone();
        let mut handled = false;
        self.api_menu_quick_move(
            [pos.0, pos.1, pos.2],
            &mut taken,
            size,
            slot,
            &mut stack,
            &mut handled,
        );
        settle(
            at,
            handled,
            stack,
            &items,
            &mut taken.items,
            &mut self.inventory,
        );
        self.put_back_mod_block_entity(pos, taken, &items);
        handled
    }

    /// The call of `mod_menu_quick_move`: the block entity is taken out of the world (journaled
    /// first, so a refused move puts it back) and the mods answer on a copy of the slot's stack.
    fn mod_menu_quick_move_fire(
        &mut self,
        pos: Pos,
        size: usize,
        slot: usize,
        at: MenuSlot,
    ) -> Fire {
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get(&pos) else {
            return Fire::Nothing;
        };
        let held = match at {
            MenuSlot::Container(k) => m.items.get(k).cloned().flatten(),
            MenuSlot::Inventory(i) => self.inventory.get(i).cloned().flatten(),
        };
        let Some(stack) = held.filter(|s| s.count > 0) else {
            return Fire::Nothing;
        };
        self.journal_block_entity(pos);
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&pos) else {
            return Fire::Nothing;
        };
        let mut stack = Some(stack);
        let mut taken = std::mem::take(&mut **m);
        let items = taken.items.clone();
        let mut handled = false;
        let valid = self.menu_quick_move_checked(
            [pos.0, pos.1, pos.2],
            &mut taken,
            size,
            slot,
            &mut stack,
            &mut handled,
        );
        if !valid {
            self.put_back_mod_block_entity(pos, taken, &items);
            return Fire::Invalid;
        }
        if !handled {
            // No mod took the click: the game's own routing moves the stack next, so nothing the
            // listener did may stay. The block entity goes back into the world and the action
            // is cancelled, which undoes every write the listener made, to this block and to others.
            self.put_back_mod_block_entity(pos, taken, &items);
            return Fire::Untaken;
        }
        Fire::Done(Fired {
            at,
            taken,
            items,
            stack,
        })
    }

    /// `api_menu_quick_move`, saying whether the mods' answer was valid: an invalid one leaves
    /// the block entity, the stack and `handled` as they were and is refused by the decision.
    fn menu_quick_move_checked(
        &mut self,
        pos: [i32; 3],
        entity: &mut ModBlockEntity,
        container_slots: usize,
        slot: usize,
        stack: &mut Option<ItemStack>,
        handled: &mut bool,
    ) -> bool {
        use crate::game::api_values::{
            BlockEntitySnapshotError, TryIntoApi, TryIntoEngine, finish_block_entity_writeback,
        };
        if !mod_api::__private::has_part_hook(mod_api::HookId::MenuQuickMove) {
            return true;
        }
        let (Ok(mut api), Ok(mut held)) = (entity.try_into_api(), stack.try_into_api()) else {
            return true;
        };
        let revision = api.revision();
        let original = held.clone();
        let mut candidate_handled = *handled;
        self.native_menu_quick_move(
            pos,
            &mut api,
            container_slots,
            slot,
            &mut held,
            &mut candidate_handled,
        );
        let held = if held == original {
            Ok(None)
        } else {
            held.try_into_engine(self.items())
                .map(Some)
                .map_err(|_| BlockEntitySnapshotError::InvalidStack)
        };
        match finish_block_entity_writeback(
            entity,
            &api,
            revision,
            self.items(),
            "menu_quick_move",
            held,
        ) {
            Ok(held) => {
                if let Some(held) = held {
                    *stack = held;
                }
                *handled = candidate_handled;
                true
            }
            Err(_) => false,
        }
    }

    /// What `mod_menu_quick_move_fire` kept, applied: the slot, the block's items and the block
    /// entity back in the world. True when a mod took the click.
    fn mod_menu_quick_move_apply(&mut self, pos: Pos, fired: Fired) -> bool {
        let Fired {
            at,
            mut taken,
            items,
            stack,
        } = fired;
        settle(
            at,
            true,
            stack,
            &items,
            &mut taken.items,
            &mut self.inventory,
        );
        self.put_back_mod_block_entity(pos, taken, &items);
        true
    }

    /// `Slot.mayPickup` of the clicked slot, which `doClick` asks before the menu's
    /// `quickMoveStack`: a slot of a mod menu type its parts refuse (`menu_slot_rules`) asks no
    /// mod, and the game's own routing refuses it with the same answer. The answers go to that
    /// routing (`Menu::click_asked`), which does not ask again.
    fn mod_menu_quick_move_gives(
        &self,
        pos: Pos,
        menu_type: Option<&'static ModMenuType>,
        slot: usize,
        at: MenuSlot,
    ) -> (bool, Asked) {
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get(&pos) else {
            return (true, Asked::default());
        };
        let held = match at {
            MenuSlot::Container(k) => m.items.get(k).cloned().flatten(),
            MenuSlot::Inventory(i) => self.inventory.get(i).cloned().flatten(),
        };
        match (menu_type, held.filter(|s| s.count > 0)) {
            (Some(t), Some(stack)) => {
                self.mod_menu_slot_gives(t, slot, &stack, [pos.0, pos.1, pos.2], &m.items)
            }
            _ => (true, Asked::default()),
        }
    }

    /// `mod_menu_quick_move` as one decided action, with the slot rules' answers asked before it.
    /// `Some((handled, asked))`: the decision stood (a mod took the click), or no mod did and the
    /// action was cancelled with every write of the listeners undone, so the game routes the
    /// click (`handled` false), or the slot's rules refused the pickup before any mod was asked;
    /// `None`: it was refused (stale, an invalid answer, a failing listener, a busy menu) and
    /// nothing changed.
    #[allow(clippy::too_many_arguments)]
    fn decided_mod_quick_move(
        &mut self,
        conn: u32,
        pos: Pos,
        size: usize,
        menu_type: Option<&'static ModMenuType>,
        slot: usize,
        at: MenuSlot,
        window: i32,
    ) -> Option<(bool, Asked)> {
        let (actor, route) = (self.guest_actor(conn)?, self.guest_route(conn)?);
        let (gives, asked) = self.as_guest(conn, |g| {
            g.mod_menu_quick_move_gives(pos, menu_type, slot, at)
        })?;
        if !gives {
            return Some((false, asked));
        }
        let admitted = Admitted::of(self, conn, pos, size, window)?;
        let fired = Rc::new(RefCell::new(None));
        let handled = Rc::new(Cell::new(false));
        let untaken = Rc::new(Cell::new(false));
        let (kept, taken, result, left) = (
            fired.clone(),
            fired.clone(),
            handled.clone(),
            untaken.clone(),
        );
        let applied = self.decided_action(
            "menu_quick_move",
            route,
            actor,
            Epoch::Player,
            // Everything the click was admitted on is still so.
            move |game| admitted.holds(game),
            move |game| match game
                .as_guest(conn, |g| g.mod_menu_quick_move_fire(pos, size, slot, at))
            {
                Some(Fire::Done(fired)) => {
                    *kept.borrow_mut() = Some(fired);
                    Reply::Allow
                }
                Some(Fire::Untaken) => {
                    left.set(true);
                    Reply::Cancel
                }
                Some(Fire::Nothing) => Reply::Allow,
                Some(Fire::Invalid) => Reply::Invalid(InvalidReason::Plan),
                None => Reply::Failed,
            },
            move |game| {
                if let Some(fired) = taken.take() {
                    result.set(
                        game.as_guest(conn, |g| g.mod_menu_quick_move_apply(pos, fired))
                            .unwrap_or(false),
                    );
                }
            },
        );
        if applied {
            Some((handled.get(), asked))
        } else {
            untaken.get().then_some((false, asked))
        }
    }
}

/// What `lan_mod_quick_move` admitted the click on, read again before the move is applied: the
/// menu, the block entity behind it (a replaced one has another generation though its values may
/// be equal) and the inputs a listener's command could change (the guest's mode and life, a drag).
/// The validity hook and the slot rules are not asked again.
pub(super) struct Admitted {
    conn: u32,
    window: i32,
    pos: Pos,
    size: usize,
    hidden: Vec<usize>,
    generation: Option<u64>,
}

impl Admitted {
    pub(super) fn of(
        game: &Game<'_>,
        conn: u32,
        pos: Pos,
        size: usize,
        window: i32,
    ) -> Option<Self> {
        let menu = game.lan.as_ref()?.hub.guest(conn)?.menu.as_ref()?;
        Some(Self {
            conn,
            window,
            pos,
            size,
            hidden: menu.hidden.clone(),
            generation: game.world.block_entities.generation(&pos),
        })
    }

    /// Whether everything admitted is still so. A block entity that is gone is not a replaced
    /// one: the move's apply puts back or drops what it took.
    pub(super) fn holds(&self, game: &Game<'_>) -> bool {
        let Some(guest) = game.lan.as_ref().and_then(|host| host.hub.guest(self.conn)) else {
            return false;
        };
        guest.menu.as_ref().is_some_and(|m| {
            m.window == self.window
                && m.slots == Slots::Block(self.pos)
                && m.size == self.size
                && m.hidden == self.hidden
                && m.drag.is_none()
        }) && !guest.spectator()
            && !guest.dead
            && game
                .world
                .block_entities
                .generation(&self.pos)
                .is_none_or(|now| Some(now) == self.generation)
    }
}

/// What the mods' call left, before it is applied.
struct Fired {
    at: MenuSlot,
    taken: ModBlockEntity,
    items: Vec<Option<ItemStack>>,
    stack: Option<ItemStack>,
}

enum Fire {
    /// Nothing to ask: the slot is empty, or the block is gone.
    Nothing,
    /// The mods' answer was invalid.
    Invalid,
    /// The mods asked took nothing: the game routes the click.
    Untaken,
    Done(Fired),
}

#[cfg(test)]
mod tests {
    use super::{MenuSlot, quick_move_slot, settle};
    use sunrise_client::items::ItemStack;

    fn stack(item: &str, count: u32) -> ItemStack {
        ItemStack {
            item: item.into(),
            count,
            damage: 0,
            extra: None,
        }
    }

    /// A quick move asks about the clicked slot only: a `generic_9x1` menu
    /// has the container's 9, then the main inventory, then the hotbar.
    #[test]
    fn a_quick_move_names_the_container_or_inventory_slot() {
        assert_eq!(
            quick_move_slot(1, 0, 0, 9, &[]),
            Some(MenuSlot::Container(0))
        );
        assert_eq!(
            quick_move_slot(1, 1, 8, 9, &[]),
            Some(MenuSlot::Container(8))
        );
        // The first main slot is inventory index 9, the last 35.
        assert_eq!(
            quick_move_slot(1, 0, 9, 9, &[]),
            Some(MenuSlot::Inventory(9))
        );
        assert_eq!(
            quick_move_slot(1, 0, 35, 9, &[]),
            Some(MenuSlot::Inventory(35))
        );
        // The hotbar comes last in the menu and first in the inventory.
        assert_eq!(
            quick_move_slot(1, 0, 36, 9, &[]),
            Some(MenuSlot::Inventory(0))
        );
        assert_eq!(
            quick_move_slot(1, 0, 44, 9, &[]),
            Some(MenuSlot::Inventory(8))
        );
    }

    /// Only a left or right shift-click on a slot the menu has is a quick
    /// move: no other click type, button, outside click or hidden slot.
    #[test]
    fn other_clicks_are_not_quick_moves() {
        // PICKUP, SWAP, CLONE, THROW, QUICK_CRAFT, PICKUP_ALL.
        for input in [0, 2, 3, 4, 5, 6] {
            assert_eq!(quick_move_slot(input, 0, 0, 9, &[]), None);
        }
        assert_eq!(quick_move_slot(1, 2, 0, 9, &[]), None);
        assert_eq!(quick_move_slot(1, -1, 0, 9, &[]), None);
        assert_eq!(quick_move_slot(1, 0, -999, 9, &[]), None);
        assert_eq!(quick_move_slot(1, 0, 45, 9, &[]), None);
        // A slot the mod's screen hides is not in the menu.
        assert_eq!(quick_move_slot(1, 0, 3, 9, &[3]), None);
        assert_eq!(
            quick_move_slot(1, 0, 4, 9, &[3]),
            Some(MenuSlot::Container(4))
        );
        // The inventory's slots are never hidden.
        assert_eq!(
            quick_move_slot(1, 0, 9, 9, &[0, 1, 2, 3, 4, 5, 6, 7, 8]),
            Some(MenuSlot::Inventory(9))
        );
    }

    /// A taken shift-click from the inventory: the clicked slot keeps what the
    /// mod left, and what it put in its block stays there.
    #[test]
    fn a_taken_click_leaves_the_remainder_in_the_clicked_slot() {
        let before = vec![None; 9];
        let mut items = before.clone();
        items[8] = Some(stack("minecraft:cobblestone", 32));
        let mut inventory = vec![None; 36];
        inventory[0] = Some(stack("minecraft:cobblestone", 64));
        let left = stack("minecraft:cobblestone", 32);
        settle(
            MenuSlot::Inventory(0),
            true,
            Some(left.clone()),
            &before,
            &mut items,
            &mut inventory,
        );
        assert_eq!(inventory[0], Some(left));
        assert_eq!(items[8], Some(stack("minecraft:cobblestone", 32)));
        // All of it taken: the slot empties.
        settle(
            MenuSlot::Inventory(0),
            true,
            None,
            &before,
            &mut items,
            &mut inventory,
        );
        assert_eq!(inventory[0], None);
    }

    /// A taken click on the block's own slot: the remainder wins over what the
    /// mod wrote to that slot, so a moved stack is never also left behind.
    #[test]
    fn the_clicked_container_slot_gets_the_remainder() {
        let before = vec![Some(stack("minecraft:dirt", 10))];
        // The mod moved the dirt to a slot past the menu's and left the clicked one as it was.
        let mut items = vec![
            Some(stack("minecraft:dirt", 10)),
            None,
            Some(stack("minecraft:dirt", 10)),
        ];
        let mut inventory = vec![None; 36];
        settle(
            MenuSlot::Container(0),
            true,
            None,
            &before,
            &mut items,
            &mut inventory,
        );
        assert_eq!(items, [None, None, Some(stack("minecraft:dirt", 10))]);
    }

    /// A click no mod took leaves the block's items as they were before the
    /// call, so a mod that moved items without taking it duplicates nothing.
    #[test]
    fn an_untaken_click_keeps_the_items_as_they_were() {
        let before = vec![Some(stack("minecraft:dirt", 10)), None];
        let mut items = vec![None, Some(stack("minecraft:dirt", 10))];
        let mut inventory = vec![None; 36];
        inventory[3] = Some(stack("minecraft:dirt", 5));
        settle(
            MenuSlot::Inventory(3),
            false,
            None,
            &before,
            &mut items,
            &mut inventory,
        );
        assert_eq!(items, before);
        assert_eq!(inventory[3], Some(stack("minecraft:dirt", 5)));
    }
}
