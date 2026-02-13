use crate::encode::component::assign::assign_indices;
use crate::encode::component::encode::encode_internal;
use crate::ir::component::idx_spaces::{ScopeId, SpaceSubtype, StoreHandle};
use crate::ir::component::scopes::{GetScopeKind, RegistryHandle};
use crate::ir::id::ComponentId;
use crate::Component;
use crate::ir::component::refs::{Depth, IndexedRef};

mod assign;
mod collect;
pub(crate) mod encode;
mod fix_indices;

/// Encode this component into its binary WebAssembly representation.
///
/// # Overview
///
/// Encoding proceeds in **three distinct phases**:
///
/// 1. **Collect**
/// 2. **Assign**
/// 3. **Encode**
///
/// These phases exist to decouple *traversal*, *index computation*, and
/// *binary emission*, which is required because instrumentation may insert,
/// reorder, or otherwise affect items that participate in index spaces.
///
/// At a high level:
///
/// - **Collect** walks the IR and records *what* needs to be encoded and
///   *where* indices are referenced.
/// - **Assign** resolves all logical references to their final concrete
///   indices after instrumentation.
/// - **Encode** emits the final binary using the resolved indices.
///
/// ---
///
/// # Phase 1: Collect
///
/// The collect phase performs a structured traversal of the component IR.
/// During this traversal:
///
/// - All items that participate in encoding are recorded into an internal
///   "plan" that determines section order and contents.
/// - Any IR node that *references indices* (e.g. types, instances, modules,
///   exports) is recorded along with enough context to later rewrite those
///   references.
///
/// No indices are rewritten during this phase.
///
/// ## Component ID Stack
///
/// Components may be arbitrarily nested. To correctly associate items with
/// their owning component, the encoder maintains a **stack of component IDs**
/// during traversal.
///
/// - When entering a component, its `ComponentId` is pushed onto the stack.
/// - When exiting, it is popped.
/// - The top of the stack always represents the *current component context*.
///
/// A **component registry** maps `ComponentId` → `&Component`, allowing the
/// encoder to recover the owning component at any point without relying on
/// pointer identity for components themselves.
///
/// ---
///
/// # Phase 2: Assign
///
/// The assign phase resolves all *logical* references recorded during collect
/// into *concrete* indices suitable for encoding.
///
/// This includes:
///
/// - Mapping original indices to their post-instrumentation positions
/// - Resolving cross-item references (e.g. type references inside signatures)
/// - Ensuring index spaces are internally consistent
///
/// ## Scope Stack
///
/// Many IR nodes are scoped (for example, nested types, instances, or
/// component-local definitions). During traversal, the encoder maintains a
/// **stack of scopes** representing the current lexical and component nesting.
///
/// - Entering a scoped node pushes a new scope.
/// - Exiting the node pops the scope.
/// - At any point, the scope stack represents the active lookup context.
///
/// ## Scope Registry
///
/// Because scoped nodes may be deeply nested and arbitrarily structured,
/// scopes are not looked up via traversal position alone.
///
/// Instead, the encoder uses a **scope registry**, which maps the *identity*
/// of an IR node to its associated scope.
///
/// - Scoped IR nodes are stored behind stable pointers (e.g. `Box<T>`).
/// - These pointers are registered exactly once when the IR is built.
/// - During assign, any IR node can query the registry to recover its scope
///   in O(1) time.
///
/// This allows index resolution to be:
///
/// - Independent of traversal order
/// - Robust to instrumentation
/// - Safe against reordering or insertion of unrelated nodes
///
/// ---
///
/// # Phase 3: Encode
///
/// Once all indices have been assigned, the encode phase performs a final
/// traversal and emits the binary representation.
///
/// At this point:
///
/// - No structural mutations occur
/// - All index values are final
/// - Encoding is a pure, deterministic process
///
/// The encoder follows the previously constructed plan to emit sections
/// in the correct order and format.
///
/// ---
///
/// # Design Notes
///
/// This three-phase architecture ensures that:
///
/// - Instrumentation can freely insert or modify IR before encoding
/// - Index correctness is guaranteed before any bytes are emitted
/// - Encoding logic remains simple and local
///
/// The use of component IDs, scope stacks, and a scope registry allows the
/// encoder to handle arbitrarily nested components and scoped definitions
/// without relying on fragile positional assumptions.
///
/// # Panics
///
/// This method may panic if:
///
/// - The component registry is inconsistent
/// - A scoped IR node is missing from the scope registry
/// - Index resolution encounters an unresolved reference
///
/// These conditions indicate an internal bug or invalid IR construction.
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
    debug_assert_eq!(1, ctx.space_stack.stack.len());
    let bytes = encode_internal(comp, &plan, &mut ctx);
    bytes.finish()
}

#[derive(Clone)]
pub(crate) struct SpaceStack {
    pub(crate) stack: Vec<ScopeId>,
}
impl SpaceStack {
    fn new(outermost_id: ScopeId) -> Self {
        Self {
            stack: vec![outermost_id],
        }
    }
    fn curr_space_id(&self) -> ScopeId {
        self.stack.last().cloned().unwrap()
    }
    fn space_at_depth(&self, depth: &Depth) -> ScopeId {
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

    pub fn enter_space(&mut self, id: ScopeId) {
        self.stack.push(id)
    }

    pub fn exit_space(&mut self) -> ScopeId {
        debug_assert!(
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
    fn maybe_enter_scope<T: GetScopeKind>(&mut self, node: &T) {
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            self.space_stack.enter_space(scope_entry.space);
        }
    }
    fn maybe_exit_scope<T: GetScopeKind>(&mut self, node: &T) {
        if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
            // Exit the nested index space...should be equivalent to the ID
            // of the scope that was entered by this node
            let exited_from = self.space_stack.exit_space();
            debug_assert_eq!(scope_entry.space, exited_from);
        }
    }
    fn enter_comp_scope(&mut self, comp_id: ComponentId) {
        let Some(scope_id) = self.registry.borrow().scope_of_comp(comp_id) else {
            panic!("no scope found for component {:?}", comp_id);
        };
        self.space_stack.enter_space(scope_id);
    }
    fn exit_comp_scope(&mut self, comp_id: ComponentId) {
        let Some(scope_id) = self.registry.borrow().scope_of_comp(comp_id) else {
            panic!("no scope found for component {:?}", comp_id);
        };
        let exited_from = self.space_stack.exit_space();
        debug_assert_eq!(scope_id, exited_from);
    }

    fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
        let scope_id = self.space_stack.space_at_depth(&r.depth);
        self.store
            .borrow()
            .scopes
            .get(&scope_id)
            .unwrap()
            .lookup_actual_id_or_panic(r)
    }

    fn index_from_assumed_id(&mut self, r: &IndexedRef) -> (SpaceSubtype, usize, Option<usize>) {
        let scope_id = self.space_stack.space_at_depth(&r.depth);
        self.store
            .borrow_mut()
            .scopes
            .get_mut(&scope_id)
            .unwrap()
            .index_from_assumed_id(r)
    }
}
