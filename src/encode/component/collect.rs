use std::collections::HashMap;
use std::process::id;
use wasmparser::{CanonicalFunction, CanonicalOption, ComponentType, CoreType};
use crate::Component;
use crate::ir::component::idx_spaces::IdxSpaces;
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
/// # Example
///
/// ```rust
/// let ptr: *const CanonicalFunction = func as *const _;
/// unsafe {
///     let func_ref: &CanonicalFunction = &*ptr;
///     func_ref.encode(&indices, &mut out);
/// }
/// ```
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

    // Type(&'a TypeDef),
    CanonicalFunc { node: *const CanonicalFunction, idx: usize },
    CoreType { node: *const CoreType<'a>, idx: usize },
    CompType { node: *const ComponentType<'a>, idx: usize },


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
    core_types: HashMap<*const CoreType<'a>, usize>,
    comp_types: HashMap<*const ComponentType<'a>, usize>,
    canon_funcs: HashMap<*const CanonicalFunction, usize>,

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
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.components.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first

        // -- the modules
        for (idx, m) in self.modules.iter().enumerate() {
            todo!()
        }

        // -- the aliases
        for (idx, a) in self.alias.items.iter().enumerate() {
            todo!()
        }

        // -- the core types
        for (idx, t) in self.core_types.iter().enumerate() {
            t.collect(idx, ctx, &self);
        }

        // -- the comp types
        for (idx, t) in self.component_types.items.iter().enumerate() {
            todo!()
        }

        // -- the imports
        for (idx, i) in self.imports.iter().enumerate() {
            todo!()
        }

        // -- the instances
        for (idx, i) in self.instances.iter().enumerate() {
            todo!()
        }

        // -- the comp instances
        for (idx, i) in self.component_instance.iter().enumerate() {
            todo!()
        }

        // -- the canonical functions
        for (idx, f) in self.canons.items.iter().enumerate() {
            f.collect(idx, ctx, &self);
        }

        // -- the nested components
        for (idx, c) in self.components.iter().enumerate() {
            let mut subctx = CollectCtx::new(c);
            c.collect(idx, &mut subctx, &self);

            // TODO -- do i need a guard here?
            ctx.plan.items.push(ComponentItem::Component {
                node: c as *const _,
                plan: subctx.plan,
                idx,
                indices: subctx.indices
            })
        }

        // -- the custom sections
        for (idx, s) in self.custom_sections.iter().enumerate() {
            s.collect(idx, ctx, &self);
            panic!()
        }


        // TODO -- finish collecting dependencies

        // assign a temporary index during collection
        // let idx = ctx.plan.items.len() as u32;
        ctx.seen.components.insert(ptr, idx);

        // TODO: I don't think I need this since everything I need is inside
        //       the ctx.plan
        // push to ordered plan
        // ctx.plan.items.push(Com::Component(ptr));
    }
}

impl<'a> Collect<'a> for CanonicalFunction {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.canon_funcs.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first
        match &self {
            CanonicalFunction::Lift { core_func_index, type_index, options } => {
                comp.canons.items[*core_func_index as usize].collect(*core_func_index as usize, ctx, comp);
                comp.component_types.items[*type_index as usize].collect(*type_index as usize, ctx, comp);

                for (idx, opt) in options.iter().enumerate() {
                    opt.collect(idx, ctx, comp);
                }
            }
            CanonicalFunction::Lower { func_index, options } => {
                comp.canons.items[*func_index as usize].collect(*func_index as usize, ctx, comp);

                for (idx, opt) in options.iter().enumerate() {
                    opt.collect(idx, ctx, comp);
                }
            }
            CanonicalFunction::ResourceNew { resource } => {
                comp.component_types.items[*resource as usize].collect(*resource as usize, ctx, comp);
            }
            CanonicalFunction::ResourceDrop { resource } => {
                comp.component_types.items[*resource as usize].collect(*resource as usize, ctx, comp);
            }
            _ => todo!("Haven't implemented this yet: {self:?}"),
        }

        // assign a temporary index during collection
        // let idx = ctx.plan.items.len() as u32;
        ctx.seen.canon_funcs.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CanonicalFunc { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CoreType<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.core_types.contains_key(&ptr) {
            return;
        }

        // TODO: Collect dependencies first

        // assign a temporary index during collection
        // let idx = ctx.plan.items.len() as u32;
        ctx.seen.core_types.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CoreType { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CanonicalOption {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        todo!()
    }
}

impl<'a> Collect<'a> for ComponentType<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.comp_types.contains_key(&ptr) {
            return;
        }

        // TODO: collect dependencies first

        // assign a temporary index during collection
        // let idx = ctx.plan.items.len() as u32;
        ctx.seen.comp_types.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CompType { node: ptr, idx });
    }
}

impl<'a> Collect<'a> for CustomSection<'a> {
    fn collect(&'a self, idx: usize, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.custom_sections.contains_key(&ptr) {
            return;
        }

        // TODO: collect dependencies first

        // assign a temporary index during collection
        // let idx = ctx.plan.items.len() as u32;
        ctx.seen.custom_sections.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CustomSection { node: ptr, idx });
    }
}
