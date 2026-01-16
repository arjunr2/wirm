// I want this file to be a bunch of oneliners (easier to read)!
#[rustfmt::skip]

use crate::ir::component::idx_spaces::{ReferencedIndices, Space, SpaceSubtype};
use crate::encode::component::EncodeCtx;
use crate::ir::component::idx_spaces::Depth;
use crate::ir::component::scopes::{build_component_store, ComponentStore, GetScopeKind};
use crate::ir::component::section::ComponentSection;
use crate::ir::component::ComponentHandle;
use crate::ir::id::ComponentId;
use crate::ir::types::CustomSection;
use crate::{assert_registered, Component, Module};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration,
};

/// A trait for each IR node to implement --> The node knows how to `collect` itself.
/// Passes the collection context AND a pointer to the containing Component
trait Collect<'a> {
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx);
}

trait CollectSubItem<'a> {
    fn collect_subitem(
        &'a self,
        idx: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan>;
}

impl Component<'_> {
    /// This is the entrypoint for collecting a component!
    pub(crate) fn collect_root(&self, ctx: &mut EncodeCtx) -> ComponentPlan {
        // I'm already in the root scope of the component at this point.
        let mut collect_ctx = CollectCtx::new(self);
        let mut comp_stack = CompStack::new(self);
        self.collect(0, &mut collect_ctx, ctx); // pass self as “container”
        collect_ctx.pop_plan().unwrap()
    }
}

impl<'a> Collect<'a> for Component<'a> {
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        let ptr = self as *const _;
        if collect_ctx.seen.components.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        collect_ctx.seen.components.insert(ptr, idx);

        // Collect dependencies first (in the order of the sections)
        for (num, section) in self.sections.iter() {
            let start_idx = {
                let mut store = ctx.store.borrow_mut();
                let indices = {
                    store
                        .scopes
                        .get_mut(&ctx.space_stack.curr_space_id())
                        .unwrap()
                };
                indices.visit_section(section, *num as usize)
            };

            println!("{section:?} Collecting {num} nodes starting @{start_idx}");
            match section {
                ComponentSection::Module => {
                    collect_vec(start_idx, *num as usize, &self.modules, collect_ctx, ctx);
                }
                ComponentSection::CoreType { .. } => {
                    collect_vec(start_idx, *num as usize, &self.core_types, collect_ctx, ctx);
                }
                ComponentSection::ComponentType { .. } => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.component_types.items,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::ComponentImport => {
                    collect_vec(start_idx, *num as usize, &self.imports, collect_ctx, ctx);
                }
                ComponentSection::ComponentExport => {
                    collect_vec(start_idx, *num as usize, &self.exports, collect_ctx, ctx);
                }
                ComponentSection::ComponentInstance => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.component_instance,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::CoreInstance => {
                    collect_vec(start_idx, *num as usize, &self.instances, collect_ctx, ctx);
                }
                ComponentSection::Alias => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.alias.items,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::Canon => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.canons.items,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::ComponentStartSection => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.start_section,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::CustomSection => {
                    collect_vec(
                        start_idx,
                        *num as usize,
                        &self.custom_sections.custom_sections,
                        collect_ctx,
                        ctx,
                    );
                }
                ComponentSection::Component => {
                    // CREATES A NEW IDX SPACE SCOPE
                    assert!(start_idx + *num as usize <= self.components.len());

                    for i in 0..*num {
                        let idx = start_idx + i as usize;
                        let c = &self.components[idx];

                        // let ptr = c as *const _;
                        // // Check if i've seen this subcomponent before during MY visitation
                        // if collect_ctx.seen.component_handles.contains_key(&ptr) {
                        //     continue;
                        // }

                        // let mut subcollect_ctx = CollectCtx::new_from(collect_ctx);
                        // let mut subctx = EncodeCtx::new_sub_ctx(c, ctx);
                        collect_ctx.push_plan();
                        collect_ctx.comp_stack.push(c.id);
                        ctx.maybe_enter_scope(c);
                        c.collect(idx, collect_ctx, ctx);
                        ctx.maybe_exit_scope(c);
                        collect_ctx.comp_stack.pop();

                        // collect_ctx.seen = subcollect_ctx.seen;

                        // I want to add this subcomponent to MY plan (not the subplan)
                        let subplan = { collect_ctx.pop_plan().unwrap() };
                        collect_ctx
                            .curr_plan_mut()
                            .items
                            .push(ComponentItem::Component {
                                node: c.clone(),
                                plan: subplan,
                                idx,
                                // space_id: space.unwrap(),
                            });

                        // Remember that I've seen this component before in MY plan
                        // collect_ctx.seen.component_handles.insert(ptr, idx);
                    }
                }
            }

            // if let Some(space) = space {
            //     // Exit the nested index space...should be equivalent
            //     // to what we entered at the beginning of this function.
            //     assert_eq!(space, ctx.space_stack.exit_space());
            // }
        }
    }
}

#[rustfmt::skip]
fn collect_section<'a, N: GetScopeKind + ReferencedIndices + 'a>(
    node: &'a N,
    idx: usize,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
    create_ptr: fn(*const N) -> TrackedItem<'a>,
    create_item: fn(*const N, usize, Option<SubItemPlan>) -> ComponentItem<'a>
) {
    let ptr = node as *const _;
    let r = create_ptr(ptr);
    if collect_ctx.seen.contains_key(&r) {
        return;
    }
    // assign a temporary index during collection
    collect_ctx.seen.insert(r, idx);

    // Collect dependencies first
    ctx.maybe_enter_scope(node);
    collect_deps(node, collect_ctx, ctx);
    ctx.maybe_exit_scope(node);

    // push to ordered plan
    collect_ctx.curr_plan_mut().items.push(create_item(ptr, idx, None));
}

// fn collect_subitems<'a, N: ReferencedIndices + 'a>(
//     node: &'a N, idx: usize,
//     subspace: Option<SubSpace>,
//     subitem_order: SubItemOrder,
//     ctx: &mut CollectCtx<'a>,
//     comp: &'a Component<'a>,
//     create_ptr: fn(*const N) -> TrackedSubItem<'a>,
//     create_item: fn(*const N, usize, Option<SubSpace>, Option<SubItemOrder>) -> ComponentItem<'a>
// ) {
//     let ptr = node as *const _;
//     let r = create_ptr(ptr);
//     if ctx.seen.contains_key(&r) {
//         return;
//     }
//     // assign a temporary index during collection
//     ctx.seen.insert(r, idx);
//
//     // Collect dependencies first
//     collect_deps(node, ctx);
//
//     // push to ordered plan
//     ctx.plan.items.push(create_item(ptr, idx, subspace, subitem_order));
// }

impl<'a> Collect<'a> for Module<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_module, ComponentItem::new_module);
    }
}

impl<'a> Collect<'a> for ComponentType<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        let ptr = self as *const _;
        let r = TrackedItem::new_comp_type(ptr);
        if collect_ctx.seen.contains_key(&r) {
            return;
        }
        // assign a temporary index during collection
        collect_ctx.seen.insert(r, idx);

        let subitem_order = self.collect_subitem(idx, collect_ctx, ctx);
        collect_ctx.curr_plan_mut().items.push(ComponentItem::new_comp_type(self as *const _, idx, subitem_order));
    }
}
impl<'a> CollectSubItem<'a> for ComponentType<'a> {
    fn collect_subitem(
        &'a self,
        _: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan> {
        // Either create a new ordering context or thread through from higher up
        match self {
            ComponentType::Component(decls) => {
                ctx.maybe_enter_scope(self);
                println!(
                    "\t@collect COMP_TYPE::component ADDR: {:p}\n\t\t{self:?}",
                    self
                );
                assert_registered!(ctx.registry, self);

                let subitems = collect_subitem_vec(decls, collect_ctx, ctx);
                ctx.maybe_exit_scope(self);
                Some(subitems)
            }
            ComponentType::Instance(decls) => {
                ctx.maybe_enter_scope(self);
                println!(
                    "\t@collect COMP_TYPE::instance ADDR: {:p}\n\t\t{self:?}",
                    self
                );
                assert_registered!(ctx.registry, self);

                let subitems = collect_subitem_vec(decls, collect_ctx, ctx);
                // let mut subitems = SubItemPlan::default();
                // for (decl_idx, decl) in decls.iter().enumerate() {
                //
                //     match decl {
                //         InstanceTypeDeclaration::CoreType(ty) => ctx.maybe_enter_scope(ty),
                //         InstanceTypeDeclaration::Type(ty) => ctx.maybe_enter_scope(ty),
                //         InstanceTypeDeclaration::Alias(_)
                //         | InstanceTypeDeclaration::Export { .. } => {}
                //     }
                //     collect_subitem(decl, decl_idx, decls, &SubSpace::default(), &mut subitems, ctx, TrackedSubItem::new_inst_type_decl);
                //     match decl {
                //         InstanceTypeDeclaration::CoreType(ty) => ctx.maybe_exit_scope(ty),
                //         InstanceTypeDeclaration::Type(ty) => ctx.maybe_exit_scope(ty),
                //         InstanceTypeDeclaration::Alias(_)
                //         | InstanceTypeDeclaration::Export { .. } => {}
                //     }
                //
                //     // let ptr = decl as *const _;
                //     // let r = TrackedSubItem::InstTypeDecl(ptr);
                //     // if ctx.seen.contains_subitem_key(&r) {
                //     //     continue;
                //     // }
                //     // // assign a temporary index during collection
                //     // ctx.seen.insert_subitem(r, decl_idx);
                //     //
                //     // // Collect dependencies first
                //     // collect_subitem_deps(decl, space.clone(), ctx, decls);
                //     //
                //     // // push to ordered plan
                //     // subitems.order.push((decl_idx, None));
                // }
                ctx.maybe_exit_scope(self);
                Some(subitems)
            }
            ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {
                None
            }
        }
    }
}

impl<'a> CollectSubItem<'a> for ComponentTypeDeclaration<'a> {
    fn collect_subitem(
        &'a self,
        idx: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan> {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ty.collect_subitem(idx, collect_ctx, ctx),
            ComponentTypeDeclaration::Type(ty) => ty.collect_subitem(idx, collect_ctx, ctx),
            ComponentTypeDeclaration::Alias(_)
            | ComponentTypeDeclaration::Export { .. }
            | ComponentTypeDeclaration::Import(_) => None,
        }
    }
}

impl<'a> CollectSubItem<'a> for InstanceTypeDeclaration<'a> {
    fn collect_subitem(
        &'a self,
        idx: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan> {
        match self {
            InstanceTypeDeclaration::CoreType(ty) => ty.collect_subitem(idx, collect_ctx, ctx),
            InstanceTypeDeclaration::Type(ty) => ty.collect_subitem(idx, collect_ctx, ctx),
            InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => None,
        }
    }
}

impl<'a> CollectSubItem<'a> for CoreType<'a> {
    fn collect_subitem(
        &'a self,
        _: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan> {
        match self {
            CoreType::Module(decls) => {
                ctx.maybe_enter_scope(self);
                assert_registered!(ctx.registry, self);
                let mut subitems = SubItemPlan::default();
                for (decl_idx, decl) in decls.iter().enumerate() {
                    // TODO: To support (outer ...) maybe have this return a Vec<Ref> to collect
                    //       at this point? Then the plan would have those component-level items first!
                    collect_subitem(
                        decl,
                        decl_idx,
                        decls,
                        &mut subitems,
                        collect_ctx,
                        ctx,
                        TrackedSubItem::new_core_type_module_decl,
                    );

                    // let ptr = decl as *const _;
                    // let r = TrackedSubItem::InstTypeDecl(ptr);
                    // if ctx.seen.contains_subitem_key(&r) {
                    //     continue;
                    // }
                    // // assign a temporary index during collection
                    // ctx.seen.insert_subitem(r, decl_idx);
                    //
                    // // Collect dependencies first
                    // collect_subitem_deps(decl, space.clone(), ctx, decls);
                    //
                    // // push to ordered plan
                    // subitems.order.push((decl_idx, None));
                }
                ctx.maybe_exit_scope(self);
                Some(subitems)
            }
            CoreType::Rec(_) => None,
        }
    }
}

impl<'a> CollectSubItem<'a> for ModuleTypeDeclaration<'a> {
    fn collect_subitem(
        &'a self,
        idx: usize,
        collect_ctx: &mut CollectCtx<'a>,
        ctx: &mut EncodeCtx,
    ) -> Option<SubItemPlan> {
        match self {
            ModuleTypeDeclaration::Type(ty) => None, // TODO -- all of these need to actually be done!
            ModuleTypeDeclaration::OuterAlias { .. } => None,
            ModuleTypeDeclaration::Export { .. } | ModuleTypeDeclaration::Import(_) => None,
        }
    }
}

impl<'a> Collect<'a> for ComponentInstance<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_comp_inst, ComponentItem::new_comp_inst);
    }
}

impl<'a> Collect<'a> for CanonicalFunction {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_canon, ComponentItem::new_canon);
    }
}

impl<'a> Collect<'a> for ComponentAlias<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_alias, ComponentItem::new_alias);
    }
}

impl<'a> Collect<'a> for ComponentImport<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_import, ComponentItem::new_import);
    }
}

impl<'a> Collect<'a> for ComponentExport<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_export, ComponentItem::new_export);
    }
}

impl<'a> Collect<'a> for CoreType<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        let ptr = self as *const _;
        let r = TrackedItem::new_core_type(ptr);
        if collect_ctx.seen.contains_key(&r) {
            return;
        }
        // assign a temporary index during collection
        collect_ctx.seen.insert(r, idx);

        let subitem_order = self.collect_subitem(idx, collect_ctx, ctx);
        collect_ctx.curr_plan_mut().items.push(ComponentItem::new_core_type(self as *const _, idx, subitem_order));
    }
}

impl<'a> Collect<'a> for Instance<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_inst, ComponentItem::new_inst);
    }
}

impl<'a> Collect<'a> for CustomSection<'a> {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_custom, ComponentItem::new_custom);
    }
}

impl<'a> Collect<'a> for ComponentStartFunction {
    #[rustfmt::skip]
    fn collect(&'a self, idx: usize, collect_ctx: &mut CollectCtx<'a>, ctx: &mut EncodeCtx) {
        collect_section(self, idx, collect_ctx, ctx, TrackedItem::new_start, ComponentItem::new_start);
    }
}

fn collect_subitem_vec<'a, T: CollectSubItem<'a> + 'a>(
    all: &'a Box<[T]>,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
) -> SubItemPlan {
    let mut subitems = SubItemPlan::default();
    for (idx, item) in all.iter().enumerate() {
        subitems.push(idx, item.collect_subitem(idx, collect_ctx, ctx));
    }
    subitems
}

fn collect_vec<'a, T: Collect<'a> + 'a>(
    start: usize,
    num: usize,
    all: &'a Vec<T>,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
) {
    assert!(start + num <= all.len(), "{start} + {num} > {}", all.len());
    for i in 0..num {
        let idx = start + i;
        let item = &all[idx];

        item.collect(idx, collect_ctx, ctx);
    }
}

fn collect_deps<'a, T: ReferencedIndices + 'a>(
    item: &T,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
) {
    if let Some(refs) = item.referenced_indices(Depth::default()) {
        for r in refs.as_list().iter() {
            println!("\tLooking up: {r:?}");
            // let curr_space_id = ctx.space_stack.curr_space_id();
            let (vec, idx) = ctx.index_from_assumed_id(r);
            // let (vec, idx)= {
            //     let mut store = ctx.store.borrow_mut();
            //     let indices = { store.scopes.get_mut(&curr_space_id).unwrap() };
            //     indices.index_from_assumed_id(r)
            // };

            let comp_id = collect_ctx.comp_at(r.depth);
            let referenced_comp = collect_ctx.comp_store.get(comp_id);

            // immutable borrow ends here

            // let referenced_comp = unsafe { &*comp_ptr };

            // referenced_comp.component_types.items[idx]
            //     .collect(idx, collect_ctx, ctx);

            let space = r.space;
            match vec {
                SpaceSubtype::Main => match space {
                    Space::CompType => {
                        referenced_comp.component_types.items[idx].collect(idx, collect_ctx, ctx)
                    }
                    Space::CompInst => {
                        referenced_comp.component_instance[idx].collect(idx, collect_ctx, ctx)
                    }
                    Space::CoreInst => {
                        referenced_comp.instances[idx].collect(idx, collect_ctx, ctx)
                    }
                    Space::CoreModule => {
                        referenced_comp.modules[idx].collect(idx, collect_ctx, ctx)
                    }
                    Space::CoreType => {
                        referenced_comp.core_types[idx].collect(idx, collect_ctx, ctx)
                    }
                    Space::CompFunc | Space::CoreFunc => {
                        referenced_comp.canons.items[idx].collect(idx, collect_ctx, ctx)
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
                SpaceSubtype::Export => referenced_comp.exports[idx].collect(idx, collect_ctx, ctx),
                SpaceSubtype::Import => referenced_comp.imports[idx].collect(idx, collect_ctx, ctx),
                SpaceSubtype::Alias => {
                    referenced_comp.alias.items[idx].collect(idx, collect_ctx, ctx)
                }
                SpaceSubtype::Components => {
                    referenced_comp.components[idx].collect(idx, collect_ctx, ctx)
                }
            }
        }
    }
}

fn collect_subitem_deps<'a, T: Debug + ReferencedIndices + CollectSubItem<'a> + 'a>(
    item: &'a T,
    subitem_order: &mut SubItemPlan,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
    nodes: &'a [T],
) {
    println!("At node: {item:?}");
    if let Some(refs) = item.referenced_indices(Depth::default()) {
        for r in refs.as_list().iter() {
            if r.depth.is_inner() {
                continue;
            }
            println!("\tLooking up: {r:?}");
            let (_, idx) = {
                let mut store = ctx.store.borrow_mut();
                let scope_id = ctx.space_stack.curr_space_id();
                let indices = { store.scopes.get_mut(&scope_id).unwrap() };
                indices.index_from_assumed_id(r)
            };

            // let subspace = subspace.subspaces.get(&idx).cloned();
            let idx_order = nodes[idx].collect_subitem(idx, collect_ctx, ctx);
            // TODO: Do I actually somehow need the below??
            subitem_order.push(idx, idx_order);
        }
    }
}

fn collect_subitem<'a, N: Debug + ReferencedIndices + CollectSubItem<'a> + 'a>(
    node: &'a N,
    idx: usize,
    nodes: &'a [N],
    subitem_order: &mut SubItemPlan,
    collect_ctx: &mut CollectCtx<'a>,
    ctx: &mut EncodeCtx,
    create_ptr: fn(*const N) -> TrackedSubItem<'a>,
) {
    let ptr = node as *const _;
    let r = create_ptr(ptr);
    if collect_ctx.seen.contains_subitem_key(&r) {
        return;
    }
    // assign a temporary index during collection
    collect_ctx.seen.insert_subitem(r, idx);

    // Collect dependencies first
    collect_subitem_deps(node, subitem_order, collect_ctx, ctx, nodes);

    // push to ordered plan
    subitem_order.push(idx, None);
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
        node: ComponentHandle<'a>,
        plan: ComponentPlan<'a>,
        idx: usize,
    },
    Module {
        node: *const Module<'a>,
        idx: usize,
    },
    CompType {
        node: *const ComponentType<'a>,
        idx: usize,
        subitem_plan: Option<SubItemPlan>,
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
        subitem_plan: Option<SubItemPlan>,
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
    fn new_module(node: *const Module<'a>, idx: usize, subitem_order: Option<SubItemPlan>) -> Self {
        if subitem_order.is_some() {
            unreachable!("modules don't have subspaces!")
        }
        Self::Module { node, idx }
    }
    fn new_comp_type(
        node: *const ComponentType<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        Self::CompType {
            node,
            idx,
            // subspace,
            subitem_plan: subitem_order,
        }
    }
    fn new_comp_inst(
        node: *const ComponentInstance<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("component instances don't have subspaces!")
        }
        Self::CompInst { node, idx }
    }
    fn new_canon(
        node: *const CanonicalFunction,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("canonical funcs don't have subspaces!")
        }
        Self::CanonicalFunc { node, idx }
    }
    fn new_alias(
        node: *const ComponentAlias<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("aliases don't have subspaces!")
        }
        Self::Alias { node, idx }
    }
    fn new_import(
        node: *const ComponentImport<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("imports don't have space IDs!")
        }
        Self::Import { node, idx }
    }
    fn new_export(
        node: *const ComponentExport<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("exports don't have space IDs!")
        }
        Self::Export { node, idx }
    }
    fn new_core_type(
        node: *const CoreType<'a>,
        idx: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        Self::CoreType {
            node,
            idx,
            // subspace,
            subitem_plan: subitem_order,
        }
    }
    fn new_inst(node: *const Instance<'a>, idx: usize, subitem_order: Option<SubItemPlan>) -> Self {
        if subitem_order.is_some() {
            unreachable!("instances don't have subspaces!")
        }
        Self::Inst { node, idx }
    }
    fn new_custom(
        node: *const CustomSection<'a>,
        _: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("custom sections don't have subspaces!")
        }
        Self::CustomSection { node }
    }
    fn new_start(
        node: *const ComponentStartFunction,
        _: usize,
        subitem_order: Option<SubItemPlan>,
    ) -> Self {
        if subitem_order.is_some() {
            unreachable!("start sections don't have subspaces!")
        }
        Self::Start { node }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SubItemPlan {
    /// item index -> optional order of ITS subitems
    order: Vec<(usize, Option<SubItemPlan>)>,
    seen: HashSet<usize>,
}
impl SubItemPlan {
    pub fn order(&self) -> &[(usize, Option<SubItemPlan>)] {
        &self.order
    }
    pub fn push(&mut self, idx: usize, subplan: Option<SubItemPlan>) {
        if !self.seen.contains(&idx) {
            self.order.push((idx, subplan));
        }
        self.seen.insert(idx);
    }
}

#[derive(Debug, Default)]
pub(crate) struct ComponentPlan<'a> {
    pub(crate) items: Vec<ComponentItem<'a>>,
}

pub(crate) enum TrackedSubItem<'a> {
    CompTypeDecl(*const ComponentTypeDeclaration<'a>),
    InstTypeDecl(*const InstanceTypeDeclaration<'a>),
    CoreTypeModuleDecl(*const ModuleTypeDeclaration<'a>),
}
impl<'a> TrackedSubItem<'a> {
    fn new_comp_type_decl(node: *const ComponentTypeDeclaration<'a>) -> Self {
        Self::CompTypeDecl(node)
    }
    fn new_inst_type_decl(node: *const InstanceTypeDeclaration<'a>) -> Self {
        Self::InstTypeDecl(node)
    }
    fn new_core_type_module_decl(node: *const ModuleTypeDeclaration<'a>) -> Self {
        Self::CoreTypeModuleDecl(node)
    }
}

/// This is just used to unify the `collect` logic into a generic function.
/// Should be the same items as `ComponentItem`, but without state.
pub(crate) enum TrackedItem<'a> {
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
pub(crate) struct Seen<'a> {
    /// Points to a TEMPORARY ID -- this is just for bookkeeping, not the final ID
    /// The final ID is assigned during the "Assign" phase.
    components: HashMap<*const Component<'a>, usize>,
    component_handles: HashMap<*const ComponentHandle<'a>, usize>,
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

    comp_type_decls: HashMap<*const ComponentTypeDeclaration<'a>, usize>,
    inst_type_decls: HashMap<*const InstanceTypeDeclaration<'a>, usize>,
    core_type_module_decls: HashMap<*const ModuleTypeDeclaration<'a>, usize>,
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
    pub fn contains_subitem_key(&self, ty: &TrackedSubItem) -> bool {
        match ty {
            TrackedSubItem::CompTypeDecl(node) => self.comp_type_decls.contains_key(node),
            TrackedSubItem::InstTypeDecl(node) => self.inst_type_decls.contains_key(node),
            TrackedSubItem::CoreTypeModuleDecl(node) => {
                self.core_type_module_decls.contains_key(node)
            }
        }
    }
    pub fn insert_subitem(&mut self, ty: TrackedSubItem<'a>, idx: usize) -> Option<usize> {
        match ty {
            TrackedSubItem::CompTypeDecl(node) => self.comp_type_decls.insert(node, idx),
            TrackedSubItem::InstTypeDecl(node) => self.inst_type_decls.insert(node, idx),
            TrackedSubItem::CoreTypeModuleDecl(node) => {
                self.core_type_module_decls.insert(node, idx)
            }
        }
    }
}

struct CompStack<'a, 'b>(Vec<&'a Component<'b>>);
impl<'a, 'b> CompStack<'a, 'b> {
    fn new(comp: &'b Component<'a>) -> Self {
        Self(vec![comp])
    }
}

pub struct CollectCtx<'a> {
    pub(crate) seen: Seen<'a>,
    pub(crate) plan_stack: Vec<ComponentPlan<'a>>,
    pub(crate) comp_stack: Vec<ComponentId>,
    pub(crate) comp_store: ComponentStore<'a>,
}
impl<'a> CollectCtx<'a> {
    fn new(comp: &'a Component<'a>) -> Self {
        let comp_store = build_component_store(comp);
        Self {
            plan_stack: vec![ComponentPlan::default()],
            seen: Seen::default(),
            comp_stack: vec![comp.id],
            comp_store,
        }
    }
    fn comp_at(&self, depth: Depth) -> &ComponentId {
        self.comp_stack
            .get(self.comp_stack.len() - depth.val() as usize - 1)
            .unwrap_or_else(|| {
                panic!(
                    "couldn't find component at depth {}; this is the current component stack: {:?}",
                    depth.val(),
                    self.comp_stack
                )
            })
    }

    fn curr_plan(&'_ self) -> &ComponentPlan {
        self.plan_stack.last().unwrap()
    }
    fn curr_plan_mut(&mut self) -> &mut ComponentPlan<'a> {
        self.plan_stack.last_mut().unwrap()
    }
    fn push_plan(&mut self) {
        self.plan_stack.push(ComponentPlan::default());
    }
    fn pop_plan(&mut self) -> Option<ComponentPlan<'a>> {
        self.plan_stack.pop()
    }
}
