use std::collections::HashMap;
use std::process::id;
use wasmparser::{CanonicalFunction, CanonicalOption, ComponentType, CoreType};
use crate::Component;
use crate::encode::component::assign::{IndexMap, Indices};

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
        original_id: u32,
        indices: Indices<'a>,
        map: IndexMap, // store nested component’s IndexMap
    },

    // Type(&'a TypeDef),
    CanonicalFunc { node: *const CanonicalFunction, original_id: u32 },
    CoreType { node: *const CoreType<'a>, original_id: u32 },
    CompType { node: *const ComponentType<'a>, original_id: u32 },
    // ... add others as needed
}
impl<'a> ComponentItem<'a> {
    pub fn update_comp_metadata(&mut self, new_indices: Indices<'a>, new_map: IndexMap,) {
        if let Self::Component { indices, map, .. } = self {
            *indices = new_indices;
            *map = new_map;
        } else {
            panic!()
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct ComponentPlan<'a> {
    pub(crate) items: Vec<ComponentItem<'a>>,
}

#[derive(Default)]
struct Seen<'a> {
    /// Points to a TEMPORARY ID -- this is just for bookkeeping, not the final ID
    /// The final ID is assigned during the "Assign" phase.
    components: HashMap<*const Component<'a>, u32>,
    core_types: HashMap<*const CoreType<'a>, u32>,
    comp_types: HashMap<*const ComponentType<'a>, u32>,
    canon_funcs: HashMap<*const CanonicalFunction, u32>,
}

#[derive(Default)]
pub(crate) struct CollectCtx<'a> {
    pub(crate) plan: ComponentPlan<'a>,
    seen: Seen<'a>,
}

/// A trait for each IR node to implement --> The node knows how to `collect` itself.
/// Passes the collection context AND a pointer to the containing Component
trait Collect<'a> {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>);
}

impl Component<'_> {
    /// This is the entrypoint for collecting a component!
    pub(crate) fn collect_root<'a>(&'a self, ctx: &mut CollectCtx<'a>) {
        self.collect(0, ctx, self) // pass self as “container”
    }
}

impl<'a> Collect<'a> for Component<'a> {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.components.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first
        // TODO: for these "dependency" collection logics -- deconstruct the node (guarantees the compiler to catch new fields I need to traverse on updating to new wasmparser versions)

        // -- the types
        for (id, t) in self.core_types.iter().enumerate() {
            t.collect(id as u32, ctx, &self);
        }

        // -- the canonical functions
        for (id, f) in self.canons.iter().enumerate() {
            f.collect(id as u32, ctx, &self);
        }

        // -- the nested components
        for (id, c) in self.components.iter().enumerate() {
            let mut subctx = CollectCtx::default();
            c.collect(id as u32, &mut subctx, &self);

            ctx.plan.items.push(ComponentItem::Component {
                node: c as *const _,
                plan: subctx.plan,
                original_id: id as u32,
                indices: Indices::default(),
                map: IndexMap::default()
            })
        }

        // TODO -- finish collecting dependencies

        // assign a temporary index during collection
        let idx = ctx.plan.items.len() as u32;
        ctx.seen.components.insert(ptr, idx);

        // TODO: I don't think I need this since everything I need is inside
        //       the ctx.plan
        // push to ordered plan
        // ctx.plan.items.push(Com::Component(ptr));
    }
}

impl<'a> Collect<'a> for CanonicalFunction {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.canon_funcs.contains_key(&ptr) {
            return;
        }

        // Collect dependencies first
        match &self {
            CanonicalFunction::Lift { core_func_index, type_index, options } => {
                comp.canons[*core_func_index as usize].collect(*core_func_index, ctx, comp);
                comp.component_types[*type_index as usize].collect(*type_index, ctx, comp);

                for (id, opt) in options.iter().enumerate() {
                    opt.collect(id as u32, ctx, comp);
                }
            }
            _ => todo!()
        }

        // assign a temporary index during collection
        let idx = ctx.plan.items.len() as u32;
        ctx.seen.canon_funcs.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CanonicalFunc { node: ptr, original_id });
    }
}

impl<'a> Collect<'a> for CoreType<'a> {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.core_types.contains_key(&ptr) {
            return;
        }

        // TODO: Collect dependencies first

        // assign a temporary index during collection
        let idx = ctx.plan.items.len() as u32;
        ctx.seen.core_types.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CoreType { node: ptr, original_id });
    }
}

impl<'a> Collect<'a> for CanonicalOption {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        todo!()
    }
}

impl<'a> Collect<'a> for ComponentType<'a> {
    fn collect(&'a self, original_id: u32, ctx: &mut CollectCtx<'a>, comp: &'a Component<'a>) {
        let ptr = self as *const _;
        if ctx.seen.comp_types.contains_key(&ptr) {
            return;
        }

        // TODO: collect dependencies first

        // assign a temporary index during collection
        let idx = ctx.plan.items.len() as u32;
        ctx.seen.comp_types.insert(ptr, idx);

        // push to ordered plan
        ctx.plan.items.push(ComponentItem::CompType { node: ptr, original_id });
    }
}
