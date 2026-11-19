//! The generated Host call and editor methods preserve the creative build values on both sides.
use super::*;
use mod_api::__private::game::BackendRefOf;
use mod_api::prelude::{ItemsBackend, ItemsHost};
use mod_api::{
    Backend, BackendRef, ClientHost, CreativeModeTabInfo, CreativeModeTabs, CreativeTabChange,
    CreativeTabEdit, CreativeTabEditError, CreativeTabList, CreativeTabPermissions,
    CreativeTabPlacement, CreativeTabReadError, CreativeTabVisibility, Host, ItemStack, ServerHost,
};

struct Catalog {
    stack: ItemStack,
    edit: CreativeTabEdit,
}

impl Host for Catalog {
    fn backend(&self) -> Option<BackendRef<'_>> {
        Some(BackendRef::of(self))
    }
}
mod_api::impl_host_areas!(Catalog, except(items));
impl ItemsHost for Catalog {
    fn creative_tabs(
        &mut self,
        permissions: CreativeTabPermissions,
    ) -> Result<Arc<CreativeModeTabs>, CreativeTabReadError> {
        if permissions != CreativeTabPermissions::Operator {
            return Err(CreativeTabReadError::Reentrant);
        }
        Ok(Arc::new(CreativeModeTabs::new(
            permissions,
            vec![CreativeModeTabInfo::new(
                "test:items".into(),
                true,
                Vec::new(),
                vec![self.stack.clone()],
            )],
        )))
    }
}
impl ServerHost for Catalog {}
mod_api::impl_server_host_areas!(Catalog);
impl ClientHost for Catalog {}
mod_api::impl_client_host_areas!(Catalog);
impl Backend for Catalog {}
mod_api::impl_backend_areas!(Catalog, except(items));
impl ItemsBackend for Catalog {
    fn creative_stack_key(&self, stack: &ItemStack) -> Option<ItemStack> {
        assert_eq!(stack, &self.stack);
        Some(self.stack.clone())
    }

    fn creative_tab_entries(
        &self,
        edit: CreativeTabEdit,
        list: CreativeTabList,
    ) -> Result<Vec<ItemStack>, CreativeTabEditError> {
        assert_eq!(list, CreativeTabList::Search);
        if edit != self.edit {
            return Err(CreativeTabEditError::Stale);
        }
        Ok(vec![self.stack.clone()])
    }

    fn creative_tab_change(
        &self,
        edit: CreativeTabEdit,
        change: CreativeTabChange,
    ) -> Result<(), CreativeTabEditError> {
        assert_eq!(edit, self.edit);
        let CreativeTabChange::Insert {
            placement: CreativeTabPlacement::After(anchor),
            stack,
            visibility: CreativeTabVisibility::Both,
        } = change
        else {
            panic!("wrong edit request: {change:?}");
        };
        assert_eq!(anchor, self.stack);
        assert_eq!(stack.count(), 2);
        Err(CreativeTabEditError::Count)
    }
}

#[test]
fn creative_reads_and_scoped_edits_cross_generated_wire_calls_on_both_sides() {
    install_counter();
    let _phase = mod_api::__private::PhaseOwner::scope(mod_api::__private::ModPhase::World);
    let mut catalog = Catalog {
        stack: ItemStack::new(REGISTERED_ITEM.get().unwrap(), 17).unwrap(),
        edit: mod_api::__private::creative_tab_edit(7),
    };
    let stack = catalog.stack.clone();
    let edit = catalog.edit;
    let mut inserted = stack.clone();
    inserted.set_count(2).unwrap();
    for server in [true, false] {
        let mut frame = boundary::Frame::default();
        if server {
            frame.set_server_host_mut(&mut catalog);
        } else {
            frame.set_client_host_mut(&mut catalog);
        }
        let mut cx = boundary::Cx::default();
        cx.frames.push(frame);
        let cx = std::cell::RefCell::new(cx);
        guest::set_transport(Some(Rc::new(move |id, bytes| {
            let mut out = Writer::new();
            // SAFETY: the host and its frame outlive this synchronous transport.
            unsafe { boundary::serve(id, &mut *cx.borrow_mut(), bytes, &mut out) }.unwrap();
            out.into_bytes()
        })));
        let mut guest = guest::Guest;
        assert_eq!(
            guest.creative_tabs(CreativeTabPermissions::Normal),
            Err(CreativeTabReadError::Reentrant)
        );
        let tabs = guest
            .creative_tabs(CreativeTabPermissions::Operator)
            .unwrap();
        let tab = tabs.tab("test:items").unwrap();
        assert!(tab.display_items.is_empty());
        assert_eq!(tab.search_items, [stack.clone()]);
        assert!(tab.contains(&guest, &stack));
        assert!(tabs.tabs_showing(&guest, &stack).next().is_none());
        assert_eq!(
            edit.entries(&guest, CreativeTabList::Search).unwrap(),
            [stack.clone()]
        );
        assert_eq!(
            edit.insert(
                &guest,
                CreativeTabPlacement::After(stack.clone()),
                inserted.clone(),
                CreativeTabVisibility::Both
            ),
            Err(CreativeTabEditError::Count)
        );
        let stale = mod_api::__private::creative_tab_edit(8);
        assert_eq!(
            stale.entries(&guest, CreativeTabList::Search),
            Err(CreativeTabEditError::Stale)
        );
        guest::set_transport(None);
    }
}
