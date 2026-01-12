// I want this file to be a bunch of oneliners (easier to read)!
#[rustfmt::skip]

use crate::ir::component::idx_spaces::{ReferencedIndices, Space, SpaceSubtype};
use crate::encode::component::SpaceStack;
use crate::ir::component::idx_spaces::{SpaceId, StoreHandle};
use crate::ir::component::section::ComponentSection;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::collections::HashMap;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, CoreType, Instance,
};

/// A trait for each IR node to implement --> The node knows how to `collect` itself.
/// Passes the collection context AND a pointer to the containing Component
trait Collect<'a> {
    fn collect(
        &'a self,
        idx: usize,
        space_id: Option<SpaceId>,
        ctx: &mut CollectCtx<'a>,
        comp: &'a Component<'a>,
    );
}

impl Component<'_> {
    /// This is the entrypoint for collecting a component!
    pub(crate) fn collect_root<'a>(&'a self, ctx: &mut CollectCtx<'a>) {
        self.collect(0, Some(self.space_id), ctx, self) // pass self as “container”
    }
}

impl<'a> Collect<'a> for Component<'a> {
    fn collect(
        &'a self,
        _idx: usize,
        _: Option<SpaceId>,
        ctx: &mut CollectCtx<'a>,
        _comp: &'a Component<'a>,
    ) {
        let ptr = self as *const _;
        if ctx.seen.components.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first (in the order of the sections)
        for (num, section) in self.sections.iter() {
            let (start_idx, space) = {
                let mut store = ctx.store.borrow_mut();
                let indices = {
                    store
                        .scopes
                        .get_mut(&ctx.space_stack.curr_space_id())
                        .unwrap()
                };
                indices.visit_section(section, *num as usize)
            };

            if let Some(space) = space {
                ctx.space_stack.enter_space(space);
            }

            println!("{section:?} Collecting {num} nodes starting @{start_idx}");
            match section {
                ComponentSection::Module => {
                    collect_vec(start_idx, *num as usize, &self.modules, ctx, None, &self);
                }
                ComponentSection::CoreType(_) => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.core_types,
                        ctx,
                        space,
                        &self,
                    );
                }
                ComponentSection::ComponentType(_) => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.component_types.items,
                        ctx,
                        space,
                        &self,
                    );
                }
                ComponentSection::ComponentImport => {
                    collect_vec(start_idx, *num as usize, &self.imports, ctx, None, &self);
                }
                ComponentSection::ComponentExport => {
                    collect_vec(start_idx, *num as usize, &self.exports, ctx, None, &self);
                }
                ComponentSection::ComponentInstance => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.component_instance,
                        ctx,
                        None,
                        &self,
                    );
                }
                ComponentSection::CoreInstance => {
                    collect_vec(start_idx, *num as usize, &self.instances, ctx, None, &self);
                }
                ComponentSection::Alias => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.alias.items,
                        ctx,
                        None,
                        &self,
                    );
                }
                ComponentSection::Canon => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.canons.items,
                        ctx,
                        None,
                        &self,
                    );
                }
                ComponentSection::ComponentStartSection => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.start_section,
                        ctx,
                        None,
                        &self,
                    );
                }
                ComponentSection::CustomSection => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.custom_sections.custom_sections,
                        ctx,
                        None,
                        &self,
                    );
                }
                ComponentSection::Component(_) => {
                    // CREATES A NEW IDX SPACE SCOPE
                    assert!(start_idx + *num as usize <= self.components.len());

                    for i in 0..*num {
                        let idx = start_idx + i as usize;
                        let c = &self.components[idx];

                        let ptr = c as *const _;
                        // Check if i've seen this subcomponent before during MY visitation
                        if ctx.seen.components.contains_key(&ptr) {
                            return;
                        }

                        let mut subctx = CollectCtx::new(c);
                        c.collect(idx, space, &mut subctx, &self);

                        // I want to add this subcomponent to MY plan (not the subplan)
                        ctx.plan.items.push(ComponentItem::Component {
                            node: c as *const _,
                            plan: subctx.plan,
                            idx,
                            space_id: space.unwrap(),
                        });

                        // Remember that I've seen this component before in MY plan
                        ctx.seen.components.insert(ptr, idx);
                    }
                }
            }

            if let Some(space) = space {
                // Exit the nested index space...should be equivalent
                // to what we entered at the beginning of this function.
                assert_eq!(space, ctx.space_stack.exit_space());
            }
        }
    }
}

#[rustfmt::skip]
fn collect_section<'a, N: ReferencedIndices + 'a>(node: &'a N, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>, create_ptr: fn(*const N) -> TrackedItem<'a>, create_item: fn(*const N, usize, Option<SpaceId>) -> ComponentItem<'a>) {
    let ptr = node as *const _;
    let r = create_ptr(ptr);
    if ctx.seen.contains_key(&r) {
        return;
    }
    // assign a temporary index during collection
    ctx.seen.insert(r, idx);

    // Collect dependencies first
    if space_id.is_none() {
        collect_deps(node, ctx, comp);
    } else {
        // TODO: Do I need to handle this or not?
        // If so, I'd need to rewrite quite a bit of the IR.
        // Basically this would let me plan the order of encoding
        // items INSIDE some nested scoped index space.
        //
        // ignore for now...
    }

    // push to ordered plan
    ctx.plan.items.push(create_item(ptr, idx, space_id));
}

impl<'a> Collect<'a> for Module<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_module, ComponentItem::new_module);
    }
}

impl<'a> Collect<'a> for ComponentType<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_comp_type, ComponentItem::new_comp_type);
    }
}

impl<'a> Collect<'a> for ComponentInstance<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_comp_inst, ComponentItem::new_comp_inst);
    }
}

impl<'a> Collect<'a> for CanonicalFunction {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_canon, ComponentItem::new_canon);
    }
}

impl<'a> Collect<'a> for ComponentAlias<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_alias, ComponentItem::new_alias);
    }
}

impl<'a> Collect<'a> for ComponentImport<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_import, ComponentItem::new_import);
    }
}

impl<'a> Collect<'a> for ComponentExport<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_export, ComponentItem::new_export);
    }
}

impl<'a> Collect<'a> for CoreType<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_core_type, ComponentItem::new_core_type);
    }
}

impl<'a> Collect<'a> for Instance<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_inst, ComponentItem::new_inst);
    }
}

impl<'a> Collect<'a> for CustomSection<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_custom, ComponentItem::new_custom);
    }
}

impl<'a> Collect<'a> for ComponentStartFunction {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, space_id: Option<SpaceId>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        collect_section(self, idx, space_id, ctx, comp, TrackedItem::new_start, ComponentItem::new_start);
    }
}

fn collect_vec<'a, T: Collect<'a> + 'a>(
    start: usize,
    num: usize,
    all: &'a Vec<T>,
    ctx: &mut CollectCtx<'a>,
    space_id: Option<SpaceId>,
    comp: &'a Component<'a>,
) {
    assert!(start + num <= all.len(), "{start} + {num} > {}", all.len());
    for i in 0..num {
        let idx = start + i;
        all[idx].collect(idx, space_id, ctx, comp);
    }
}

fn collect_deps<'a, T: ReferencedIndices + 'a>(
    item: &T,
    ctx: &mut CollectCtx<'a>,
    comp: &'a Component<'a>,
) {
    if let Some(refs) = item.referenced_indices() {
        for r in refs.as_list().iter() {
            println!("\tLooking up: {r:?}");
            let curr_space_id = ctx.space_stack.curr_space_id();
            let (vec, idx) = {
                let mut store = ctx.store.borrow_mut();
                let indices = { store.scopes.get_mut(&curr_space_id).unwrap() };
                indices.index_from_assumed_id(r)
            };

            // TODO: For nested index spaces, dependencies would actually reference their own decls!
            let space = r.space;
            match vec {
                SpaceSubtype::Main => match space {
                    Space::CompType => {
                        comp.component_types.items[idx].collect(idx, None, ctx, comp)
                    }
                    Space::CompInst => comp.component_instance[idx].collect(idx, None, ctx, comp),
                    Space::CoreInst => comp.instances[idx].collect(idx, None, ctx, comp),
                    Space::CoreModule => comp.modules[idx].collect(idx, None, ctx, comp),
                    Space::CoreType => comp.core_types[idx].collect(idx, None, ctx, comp),
                    Space::CompFunc | Space::CoreFunc => {
                        comp.canons.items[idx].collect(idx, None, ctx, comp)
                    }
                    Space::CompVal
                    | Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                    // Space::NA => continue,
                },
                SpaceSubtype::Export => comp.exports[idx].collect(idx, None, ctx, comp),
                SpaceSubtype::Import => comp.imports[idx].collect(idx, None, ctx, comp),
                SpaceSubtype::Alias => comp.alias.items[idx].collect(idx, None, ctx, comp),
                SpaceSubtype::Components => comp.components[idx].collect(idx, None, ctx, comp),
            }
        }
    }
}

/// `ComponentItem` stores raw pointers to IR nodes (e.g., `CanonicalFunction`, `Module`, `Component`)
/// rather than `&T` references directly.
///
/// # Safety
///
/// This is safe under the following conditions:
///
/// 1. **The IR outlives the plan** (`'a` lifetime):
///    All IR nodes are borrowed from a buffer (e.g., the wasm module bytes) that lives at least
///    as long as the `EncodePlan<'a>` and `Indices<'a>`. Therefore, the raw pointers will always
///    point to valid memory for the lifetime `'a`.
///
/// 2. **Pointers are not mutated or deallocated**:
///    The IR is immutable, so dereferencing the pointers for read-only operations (like `encode`)
///    cannot cause undefined behavior.
///
/// 3. **Dereference only occurs inside `unsafe` blocks**:
///    Rust requires `unsafe` to dereference `*const T`. We carefully ensure that all dereferences
///    happen while the IR is still alive and valid.
///
/// 4. **Phase separation is respected**:
///    - **Collect phase** builds a linear plan of IR nodes, storing raw pointers as handles.
///    - **Assign indices phase** assigns numeric IDs to nodes in the order they appear in the plan.
///    - **Encode phase** dereferences pointers to emit bytes.
///
/// By storing raw pointers instead of `&'a T`, we avoid lifetime and variance conflicts that would
/// occur if `EncodePlan<'a>` were mutably borrowed while simultaneously pushing `&'a T` references.
///
/// The `'a` lifetime ensures the underlying IR node lives long enough, making this `unsafe`
/// dereference sound.
#[derive(Debug)]
pub(crate) enum ComponentItem<'a> {
    Component {
        node: *const Component<'a>,
        plan: ComponentPlan<'a>,
        idx: usize,
        space_id: SpaceId,
    },
    Module {
        node: *const Module<'a>,
        idx: usize,
    },
    CompType {
        node: *const ComponentType<'a>,
        idx: usize,
        space_id: Option<SpaceId>,
    },
    CompInst {
        node: *const ComponentInstance<'a>,
        idx: usize,
    },
    CanonicalFunc {
        node: *const CanonicalFunction,
        idx: usize,
    },

    Alias {
        node: *const ComponentAlias<'a>,
        idx: usize,
    },
    Import {
        node: *const ComponentImport<'a>,
        idx: usize,
    },
    Export {
        node: *const ComponentExport<'a>,
        idx: usize,
    },

    CoreType {
        node: *const CoreType<'a>,
        idx: usize,
        space_id: Option<SpaceId>,
    },
    Inst {
        node: *const Instance<'a>,
        idx: usize,
    },

    Start {
        node: *const ComponentStartFunction,
        // idx: usize,
    },
    CustomSection {
        node: *const CustomSection<'a>,
        // idx: usize,
    },
    // ... add others as needed
}
impl<'a> ComponentItem<'a> {
    fn new_module(node: *const Module<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Module { node, idx }
    }
    fn new_comp_type(
        node: *const ComponentType<'a>,
        idx: usize,
        space_id: Option<SpaceId>,
    ) -> Self {
        Self::CompType {
            node,
            idx,
            space_id,
        }
    }
    fn new_comp_inst(
        node: *const ComponentInstance<'a>,
        idx: usize,
        space_id: Option<SpaceId>,
    ) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::CompInst { node, idx }
    }
    fn new_canon(node: *const CanonicalFunction, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::CanonicalFunc { node, idx }
    }
    fn new_alias(node: *const ComponentAlias<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Alias { node, idx }
    }
    fn new_import(node: *const ComponentImport<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Import { node, idx }
    }
    fn new_export(node: *const ComponentExport<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Export { node, idx }
    }
    fn new_core_type(node: *const CoreType<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        Self::CoreType {
            node,
            idx,
            space_id,
        }
    }
    fn new_inst(node: *const Instance<'a>, idx: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Inst { node, idx }
    }
    fn new_custom(node: *const CustomSection<'a>, _: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::CustomSection { node }
    }
    fn new_start(node: *const ComponentStartFunction, _: usize, space_id: Option<SpaceId>) -> Self {
        if space_id.is_some() {
            unreachable!("modules don't have space IDs!")
        }
        Self::Start { node }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ComponentPlan<'a> {
    pub(crate) items: Vec<ComponentItem<'a>>,
}

/// This is just used to unify the `collect` logic into a generic function.
/// Should be the same items as `ComponentItem`, but without state.
enum TrackedItem<'a> {
    // unnecessary since this is handled in a non-generic way
    // Component(*const Component<'a>),
    Module(*const Module<'a>),
    CompType(*const ComponentType<'a>),
    CompInst(*const ComponentInstance<'a>),
    CanonicalFunc(*const CanonicalFunction),
    Alias(*const ComponentAlias<'a>),
    Import(*const ComponentImport<'a>),
    Export(*const ComponentExport<'a>),
    CoreType(*const CoreType<'a>),
    Inst(*const Instance<'a>),
    Start(*const ComponentStartFunction),
    CustomSection(*const CustomSection<'a>),
    // ... add others as needed
}
impl<'a> TrackedItem<'a> {
    fn new_module(node: *const Module<'a>) -> Self {
        Self::Module(node)
    }
    fn new_comp_type(node: *const ComponentType<'a>) -> Self {
        Self::CompType(node)
    }
    fn new_comp_inst(node: *const ComponentInstance<'a>) -> Self {
        Self::CompInst(node)
    }
    fn new_canon(node: *const CanonicalFunction) -> Self {
        Self::CanonicalFunc(node)
    }
    fn new_alias(node: *const ComponentAlias<'a>) -> Self {
        Self::Alias(node)
    }
    fn new_import(node: *const ComponentImport<'a>) -> Self {
        Self::Import(node)
    }
    fn new_export(node: *const ComponentExport<'a>) -> Self {
        Self::Export(node)
    }
    fn new_core_type(node: *const CoreType<'a>) -> Self {
        Self::CoreType(node)
    }
    fn new_inst(node: *const Instance<'a>) -> Self {
        Self::Inst(node)
    }
    fn new_custom(node: *const CustomSection<'a>) -> Self {
        Self::CustomSection(node)
    }
    fn new_start(node: *const ComponentStartFunction) -> Self {
        Self::Start(node)
    }
}

#[derive(Default)]
struct Seen<'a> {
    /// Points to a TEMPORARY ID -- this is just for bookkeeping, not the final ID
    /// The final ID is assigned during the "Assign" phase.
    components: HashMap<*const Component<'a>, usize>,
    modules: HashMap<*const Module<'a>, usize>,
    comp_types: HashMap<*const ComponentType<'a>, usize>,
    comp_instances: HashMap<*const ComponentInstance<'a>, usize>,
    canon_funcs: HashMap<*const CanonicalFunction, usize>,

    aliases: HashMap<*const ComponentAlias<'a>, usize>,
    imports: HashMap<*const ComponentImport<'a>, usize>,
    exports: HashMap<*const ComponentExport<'a>, usize>,

    core_types: HashMap<*const CoreType<'a>, usize>,
    instances: HashMap<*const Instance<'a>, usize>,

    start: HashMap<*const ComponentStartFunction, usize>,
    custom_sections: HashMap<*const CustomSection<'a>, usize>,
}
impl<'a> Seen<'a> {
    pub fn contains_key(&self, ty: &TrackedItem) -> bool {
        match ty {
            TrackedItem::Module(node) => self.modules.contains_key(node),
            TrackedItem::CompType(node) => self.comp_types.contains_key(node),
            TrackedItem::CompInst(node) => self.comp_instances.contains_key(node),
            TrackedItem::CanonicalFunc(node) => self.canon_funcs.contains_key(node),
            TrackedItem::Alias(node) => self.aliases.contains_key(node),
            TrackedItem::Import(node) => self.imports.contains_key(node),
            TrackedItem::Export(node) => self.exports.contains_key(node),
            TrackedItem::CoreType(node) => self.core_types.contains_key(node),
            TrackedItem::Inst(node) => self.instances.contains_key(node),
            TrackedItem::Start(node) => self.start.contains_key(node),
            TrackedItem::CustomSection(node) => self.custom_sections.contains_key(node),
        }
    }
    pub fn insert(&mut self, ty: TrackedItem<'a>, idx: usize) -> Option<usize> {
        match ty {
            TrackedItem::Module(node) => self.modules.insert(node, idx),
            TrackedItem::CompType(node) => self.comp_types.insert(node, idx),
            TrackedItem::CompInst(node) => self.comp_instances.insert(node, idx),
            TrackedItem::CanonicalFunc(node) => self.canon_funcs.insert(node, idx),
            TrackedItem::Alias(node) => self.aliases.insert(node, idx),
            TrackedItem::Import(node) => self.imports.insert(node, idx),
            TrackedItem::Export(node) => self.exports.insert(node, idx),
            TrackedItem::CoreType(node) => self.core_types.insert(node, idx),
            TrackedItem::Inst(node) => self.instances.insert(node, idx),
            TrackedItem::Start(node) => self.start.insert(node, idx),
            TrackedItem::CustomSection(node) => self.custom_sections.insert(node, idx),
        }
    }
}

pub(crate) struct CollectCtx<'a> {
    pub(crate) plan: ComponentPlan<'a>,
    seen: Seen<'a>,

    pub(crate) space_stack: SpaceStack,
    pub(crate) store: StoreHandle,
}
impl CollectCtx<'_> {
    pub fn new(comp: &Component) -> Self {
        Self {
            plan: ComponentPlan::default(),
            seen: Seen::default(),

            space_stack: SpaceStack::new(comp.space_id),
            store: comp.index_store.clone(),
        }
    }

    fn in_space(&self, space_id: Option<SpaceId>) -> bool {
        if let Some(space_id) = space_id {
            return self.space_stack.curr_space_id() == space_id;
        }
        true
    }
}
