# Reading tags in custom loot evaluation

Registered `ModLootFunction` and `ModLootCondition` codecs read
`LootTypeContext::tags() -> &dyn Tags` alongside their owned context and candidate
snapshots. This is a read-only view of the merged tags used by the current loot
evaluator. The context supplies only its snapshot, tags and selected random.

Resolve an `Identifier` with `context.tags().key(&id)`, retain the resulting `TagKey`,
and call `is_in_tag(item, tag)` for item membership. `items(tag)`, `blocks(tag)`
and `fluids(tag)` return ordered members by key. There is no operation to
enumerate every tag. Unknown tags return `Some(false)` or `Some(Vec::new())`;
unavailable key mappings return `None`. A retained key keeps its identity across
reloads, but its members are read from the current evaluation's tables.

Neither
callback gains a mutable game host or an opportunity to change tags. No tag
snapshot is copied when no codec with the evaluated type id is registered.
