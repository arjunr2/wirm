use crate::encode::component::assign::assign_indices;
use crate::encode::component::encode::encode_internal;
use crate::ir::component::idx_spaces::{Depth, IndexedRef, SpaceId, SpaceSubtype, StoreHandle};
use crate::ir::component::scopes::{build_component_store, GetScopeKind, RegistryHandle};
use crate::Component;

mod assign;
mod collect;
pub(crate) mod encode;
mod fix_indices;
// mod encode_bk;

/// Encode this IR into a WebAssembly binary.
/// Encoding a component gets split into 3 phases (the first two are for planning, the final
/// phase is to actually perform the encoding)
/// 1. Collect phase
///  - Walk dependencies
///  - Deduplicate using Seen
///  - Produce topologically sorted plan
/// 2. Index assignment phase
///  - Assign sequential indices after collection
///  - Separate from bytes
/// 3. Encoding phase
///  - Emit bytes using indices
///  - No recursion needed, all references are guaranteed to be valid
///
/// ## Index resolution behavior
///
/// During encoding, this function performs **basic index collection and
/// reindexing** within a single, well-defined index space. This includes
/// adjusting indices to account for:
///
/// - Newly inserted items
/// - Removed or reordered items
/// - Flat, non-nested index spaces
///
/// However, the encoder **does not attempt to resolve or rewrite indices whose
/// meaning depends on nested index-space scopes**, such as those introduced by
/// the WebAssembly component model.
///
/// In particular, indices whose correctness depends on entering or exiting
/// nested scopes are assumed to already be valid in the context where they
/// appear.
///
/// ### Examples
///
/// #### Supported: flat reindexing within a single scope
///
/// If instrumentation inserts a new component function before an existing one:
///
/// ```wat
/// ;; Original
/// (component
///   (func $f0)
///   (func $f1)
/// )
///
/// ;; After instrumentation
/// (component
///   (func $new)
///   (func $f0)
///   (func $f1)
/// )
/// ```
///
/// The encoder will automatically reindex references to `$f0` and `$f1` to
/// account for the inserted function.
///
/// #### Not supported: scope-dependent index resolution
///
/// ```wat
/// (component
///   (component $inner
///     (func $f)
///   )
///   (instance $i (instantiate $inner))
///   (export "f" (func $i "f"))
/// )
/// ```
///
/// In this example, the meaning of the exported function index depends on:
///
/// - Which component is instantiated
/// - Which instance scope is active
///
/// The encoder does **not** attempt to determine or rewrite such indices.
/// The IR is assumed to already reference the correct function in the correct
/// scope.
///
/// #### Not supported: nested core/module scopes
///
/// ```wat
/// (component
///   (component
///     (core module
///       (func $f)
///     )
///   )
///   (canon lift (core func $f))
/// )
/// ```
///
/// If `$f` is defined inside a nested core module scope, the encoder assumes
/// that any reference to it already uses the correct index for that scope.
///
/// ### Summary
///
/// - Flat, single-scope reindexing is handled automatically.
/// - Nested or scope-dependent index resolution is not.
/// - Earlier phases are responsible for ensuring scope-correct indices.
///
/// This design keeps encoding deterministic and avoids implicit cross-scope
/// rewriting that would be difficult to reason about or validate.
pub fn encode(comp: &Component) -> Vec<u8> {
    // Phase 1: Collect
    let mut ctx = EncodeCtx::new(comp);
    let mut plan = comp.collect_root(&mut ctx);

    // Phase 2: Assign indices
    {
        let mut store = ctx.store.borrow_mut();
        store.reset_indices();
    }
    assign_indices(&mut plan, &mut ctx);

    // Phase 3: Encode (pass in the root-level component's plan, assigned indices, and original->new index map)
    assert_eq!(1, ctx.space_stack.stack.len());
    let bytes = encode_internal(&comp, &plan, &mut ctx);
    bytes.finish()
}

#[derive(Clone)]
pub(crate) struct SpaceStack {
    pub(crate) stack: Vec<SpaceId>,
}
impl SpaceStack {
    fn new(outermost_id: SpaceId) -> Self {
        Self {
            stack: vec![outermost_id],
        }
    }
    fn curr_space_id(&self) -> SpaceId {
        self.stack.last().cloned().unwrap()
    }
    fn space_at_depth(&self, depth: &Depth) -> SpaceId {
        *self
            .stack
            .get(self.stack.len() - depth.val() as usize - 1)
            .unwrap_or_else(|| {
                panic!(
                    "couldn't find scope at depth {}; this is the current scope stack: {:?}",
                    depth.val(),
                    self.stack
                )
            })
    }

    pub fn enter_space(&mut self, id: SpaceId) {
        self.stack.push(id)
    }

    pub fn exit_space(&mut self) -> SpaceId {
        assert!(
            self.stack.len() >= 2,
            "Trying to exit the index space scope when there isn't an outer!"
        );
        self.stack.pop().unwrap()
    }
}

pub(crate) struct EncodeCtx {
    pub(crate) space_stack: SpaceStack,
    pub(crate) registry: RegistryHandle,
    pub(crate) store: StoreHandle,
}
impl EncodeCtx {
    pub fn new(comp: &Component) -> Self {
        Self {
            space_stack: SpaceStack::new(comp.space_id),
            registry: comp.scope_registry.clone(),
            store: comp.index_store.clone(),
        }
    }

    pub fn new_sub_ctx(comp: &Component, outer: &EncodeCtx) -> Self {
        let mut new_stack = outer.space_stack.clone();
        new_stack.enter_space(comp.space_id);

        Self {
            space_stack: new_stack,
            registry: comp.scope_registry.clone(),
            store: comp.index_store.clone(),
        }
    }

    fn in_space(&self, space_id: Option<SpaceId>) -> bool {
        if let Some(space_id) = space_id {
            return self.space_stack.curr_space_id() == space_id;
        }
        true
    }

    fn maybe_enter_scope<T: GetScopeKind>(&mut self, node: &T) {
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            println!(">>> ENTER scope{}", scope_entry.space);
            self.space_stack.enter_space(scope_entry.space);
        }
    }
    fn maybe_exit_scope<T: GetScopeKind>(&mut self, node: &T) {
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            println!("<<< EXIT scope{}", scope_entry.space);
            // Exit the nested index space...should be equivalent to the ID
            // of the scope that was entered by this node
            debug_assert_eq!(scope_entry.space, self.space_stack.exit_space());
        }
    }

    fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
        let scope_id = self.space_stack.space_at_depth(&r.depth);
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .lookup_actual_id_or_panic(&r)
    }

    fn index_from_assumed_id(&self, r: &IndexedRef) -> (SpaceSubtype, usize) {
        let scope_id = self.space_stack.space_at_depth(&r.depth);
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .index_from_assumed_id(&r)
    }
}
