use std::collections::HashMap;
use wasmparser::{CanonicalFunction, CanonicalOption, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentType, ComponentTypeRef, CoreType, Instance};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::{ExternalItemKind, IdxSpaces, SpaceSubtype};
use crate::ir::section::ComponentSection;
use crate::ir::types::CustomSection;

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
        idx: usize,                 // TODO: I don't think I need idx here!
        // indices: Indices<'a>,
        indices: IdxSpaces, // store nested component’s IndexMap
    },
    Module {node: *const Module<'a>, idx: usize },
    CompType { node: *const ComponentType<'a>, idx: usize },
    CompInst { node: *const ComponentInstance<'a>, idx: usize },
    CanonicalFunc { node: *const CanonicalFunction, idx: usize },

    Alias { node: *const ComponentAlias<'a>, idx: usize },
    Import { node: *const ComponentImport<'a>, idx: usize },
    Export { node: *const ComponentExport<'a>, idx: usize },

    CoreType { node: *const CoreType<'a>, idx: usize },
    Inst { node: *const Instance<'a>, idx: usize },

    CustomSection { node: *const CustomSection<'a>, idx: usize },
    // ... add others as needed
}
// impl<'a> ComponentItem<'a> {
//     pub fn update_comp_metadata(&mut self, new_indices: Indices<'a>, new_map: IdxSpaces) {
//         if let Self::Component { indices, idx_spaces: map, .. } = self {
//             *indices = new_indices;
//             *map = new_map;
//         } else {
//             panic!()
//         }
//     }
// }

#[derive(Debug, Default)]
pub(crate) struct ComponentPlan<'a> {
    pub(crate) items: Vec<ComponentItem<'a>>,
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

    custom_sections: HashMap<*const CustomSection<'a>, usize>
}

#[derive(Default)]
pub(crate) struct CollectCtx<'a> {
    pub(crate) plan: ComponentPlan<'a>,
    pub(crate) indices: IdxSpaces,
    seen: Seen<'a>,
}
impl CollectCtx<'_> {
    pub fn new(comp: &Component) -> Self {
        Self {
            indices: comp.indices.clone(),
            ..Default::default()
        }
    }
}

/// A trait for each IR node to implement --> The node knows how to `collect` itself.
/// Passes the collection context AND a pointer to the containing Component
trait Collect<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>);
}

impl Component<'_> {
    /// This is the entrypoint for collecting a component!
    pub(crate) fn collect_root<'a>(&'a self, ctx: &mut CollectCtx<'a>) {
        self.collect(0, ctx, self) // pass self as “container”
    }
}

impl<'a> Collect<'a> for Component<'a> {
    fn collect(&'a self, _idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.components.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first (in the order of the sections)
        for (num, section) in self.sections.iter() {
            let start_idx = ctx.indices.visit_section(section, *num as usize);

            match section {
                ComponentSection::Module => {
                    collect_vec(start_idx, *num as usize, &self.modules, ctx, &self);
                }
                ComponentSection::CoreType => {
                    collect_vec(start_idx, *num as usize, &self.core_types, ctx, &self);
                }
                ComponentSection::ComponentType => {
                    collect_vec(start_idx, *num as usize, &self.component_types.items, ctx, &self);
                }
                ComponentSection::ComponentImport => {
                    collect_vec(start_idx, *num as usize, &self.imports, ctx, &self);
                }
                ComponentSection::ComponentExport => {
                    collect_vec(start_idx, *num as usize, &self.exports, ctx, &self);
                }
                ComponentSection::ComponentInstance => {
                    collect_vec(start_idx, *num as usize, &self.component_instance, ctx, &self);
                }
                ComponentSection::CoreInstance => {
                    collect_vec(start_idx, *num as usize, &self.instances, ctx, &self);
                }
                ComponentSection::Alias => {
                    collect_vec(start_idx, *num as usize, &self.alias.items, ctx, &self);
                }
                ComponentSection::Canon => {
                    collect_vec(start_idx, *num as usize, &self.canons.items, ctx, &self);
                }
                ComponentSection::ComponentStartSection => {
                    todo!()
                }
                ComponentSection::CustomSection => {
                    collect_vec(start_idx, *num as usize, &self.custom_sections.custom_sections, ctx, &self);
                }
                ComponentSection::Component => {
                    assert!(start_idx + *num as usize <= self.components.len());

                    for i in 0..*num {
                        let idx = start_idx + i as usize;
                        let c = &self.components[idx];

                        let ptr = self as *const _;
                        // Check if i've seen this subcomponent before during MY visitation
                        if ctx.seen.components.contains_key(&ptr) {
                            return;
                        }

                        let mut subctx = CollectCtx::new(c);
                        c.collect(idx, &mut subctx, &self);

                        // I want to add this subcomponent to MY plan (not the subplan)
                        ctx.plan.items.push(ComponentItem::Component {
                            node: c as *const _,
                            plan: subctx.plan,
                            idx,
                            indices: subctx.indices
                        });

                        // Remember that I've seen this component before in MY plan
                        ctx.seen.components.insert(ptr, idx);
                    }
                }
            }
        }
    }
}

impl<'a> Collect<'a> for Module<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.modules.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.modules.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::Module { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for ComponentType<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.comp_types.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.comp_types.insert(ptr, idx);

        // TODO: collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CompType { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for ComponentInstance<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.comp_instances.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.comp_instances.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CompInst { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CanonicalFunction {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.canon_funcs.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.canon_funcs.insert(ptr, idx);

        // let kind = ExternalItemKind::from(self);
        // Collect dependencies first
        match &self {
            CanonicalFunction::Lift { core_func_index, type_index, options } => {
                let (canon_vec, canon_idx) = ctx.indices.index_from_assumed_id(&ComponentSection::Canon, &ExternalItemKind::CoreFunc, *core_func_index as usize);
                // assert!(matches!(ty, SpaceSubtype::Main), "didn't match {ty:?}");
                let (ty_vec, ty_idx) = ctx.indices.index_from_assumed_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *type_index as usize);
                // assert!(matches!(ty, SpaceSubtype::Main));

                match canon_vec {
                    SpaceSubtype::Export => comp.exports[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Import => comp.imports[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Components |
                    SpaceSubtype::Main => panic!("Shouldn't get here"),
                }

                match ty_vec {
                    SpaceSubtype::Main => comp.component_types.items[ty_idx].collect(ty_idx, ctx, comp),
                    SpaceSubtype::Export => comp.exports[ty_idx].collect(ty_idx, ctx, comp),
                    SpaceSubtype::Import => comp.imports[ty_idx].collect(ty_idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[ty_idx].collect(ty_idx, ctx, comp),
                    SpaceSubtype::Components => panic!("Shouldn't get here"),
                }

                for (idx, opt) in options.iter().enumerate() {
                    opt.collect(idx, ctx, comp);
                }
            }
            CanonicalFunction::Lower { func_index, options } => {
                let (canon_vec, canon_idx) = ctx.indices.index_from_assumed_id(&ComponentSection::Canon, &ExternalItemKind::CompFunc, *func_index as usize);
                // assert!(matches!(ty, SpaceSubtype::Main));
                // comp.canons.items[canon_idx].collect(canon_idx, ctx, comp);

                match canon_vec {
                    SpaceSubtype::Export => comp.exports[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Import => comp.imports[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[canon_idx].collect(canon_idx, ctx, comp),
                    SpaceSubtype::Components |
                    SpaceSubtype::Main => panic!("Shouldn't get here"),
                }

                for (idx, opt) in options.iter().enumerate() {
                    opt.collect(idx, ctx, comp);
                }
            }
            CanonicalFunction::ResourceNew { resource } => {
                let (ty, ty_idx) = ctx.indices.index_from_assumed_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize);
                assert!(matches!(ty, SpaceSubtype::Main));

                comp.component_types.items[ty_idx].collect(ty_idx, ctx, comp);
            }
            CanonicalFunction::ResourceDrop { resource } => {
                let (ty, ty_idx) = ctx.indices.index_from_assumed_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *resource as usize);
                assert!(matches!(ty, SpaceSubtype::Main));
                comp.component_types.items[ty_idx].collect(ty_idx, ctx, comp);
            }
            _ => todo!("Haven't implemented this yet: {self:?}"),
        }


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CanonicalFunc { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for ComponentAlias<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.aliases.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.aliases.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::Alias { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for ComponentImport<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.imports.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.imports.insert(ptr, idx);

        // TODO: Collect dependencies first
        match &self.ty {
            // The reference is to a core module type.
            // The index is expected to be core type index to a core module type.
            ComponentTypeRef::Module(id) => {
                let (ty, idx) = ctx.indices.index_from_assumed_id(&ComponentSection::CoreType, &ExternalItemKind::NA, *id as usize);
                assert!(matches!(ty, SpaceSubtype::Main));

                comp.core_types[idx].collect(idx, ctx, comp);
            }
            ComponentTypeRef::Func(id) => {}
            ComponentTypeRef::Value(old_id) => {}
            ComponentTypeRef::Type(old_id) => {}
            ComponentTypeRef::Instance(id) => {}
            ComponentTypeRef::Component(id) => {}
        }


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::Import { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for ComponentExport<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.exports.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.exports.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::Export { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CoreType<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.core_types.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.core_types.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CoreType { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for Instance<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.instances.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.instances.insert(ptr, idx);

        // TODO: Collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::Inst { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CustomSection<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, _comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.custom_sections.contains_key(&ptr) {
            return;
        }
        // assign a temporary index during collection
        ctx.seen.custom_sections.insert(ptr, idx);

        // TODO: collect dependencies first


        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CustomSection { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CanonicalOption {
    fn collect(&'a self, _idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        match self {
            CanonicalOption::Memory(id) => {
                let (mem_vec, idx) = ctx.indices.index_from_assumed_id(&ComponentSection::Canon, &ExternalItemKind::CoreMemory, *id as usize);

                match mem_vec {
                    SpaceSubtype::Import => comp.imports[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Components |
                    SpaceSubtype::Export |
                    SpaceSubtype::Main => panic!("Shouldn't get here"),
                }
            }
            CanonicalOption::PostReturn(id) |
            CanonicalOption::Callback(id) |
            CanonicalOption::Realloc(id) => {
                let (mem_vec, idx) = ctx.indices.index_from_assumed_id(&ComponentSection::Canon, &ExternalItemKind::CoreFunc, *id as usize);

                match mem_vec {
                    // TODO: This could collect something 2x?
                    // Does `seen` check avoid this? -- might need to collect one at a time instead?
                    SpaceSubtype::Main => comp.canons.items[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Import => comp.imports[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Components |
                    SpaceSubtype::Export => panic!("Shouldn't get here"),
                }
            }
            CanonicalOption::CoreType(id) => {
                let (mem_vec, idx) = ctx.indices.index_from_assumed_id(&ComponentSection::CoreType, &ExternalItemKind::NA, *id as usize);

                match mem_vec {
                    SpaceSubtype::Import => comp.imports[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Alias => comp.alias.items[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Main => comp.core_types[idx].collect(idx, ctx, comp),
                    SpaceSubtype::Components |
                    SpaceSubtype::Export => panic!("Shouldn't get here"),
                }
            }
            CanonicalOption::UTF8 |
            CanonicalOption::UTF16 |
            CanonicalOption::CompactUTF16 |
            CanonicalOption::Async |
            CanonicalOption::Gc => {}   // do nothing
        }
    }
}

fn collect_vec<'a, T: Collect<'a> + 'a>(start: usize, num: usize, all: &'a Vec<T>, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
    assert!(start + num <= all.len());
    for i in 0..num {
        let idx = start + i;
        all[idx].collect(idx, ctx, comp);
    }
}
