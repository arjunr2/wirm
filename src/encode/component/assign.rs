use crate::encode::component::collect::{ComponentItem, ComponentPlan};
use crate::ir::component::idx_spaces::{IdxSpaces, IndexSpaceOf};
use crate::ir::section::ComponentSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentImport, ComponentInstance, ComponentType, CoreType,
    Instance,
};

/// # Phase 2: ASSIGN #
/// ## Safety of Alias Index Assignment
///
/// During the assign phase, the encoder determines the final index
/// (or "actual id") of each component item based on the order in which
/// items will be emitted into the binary. This includes alias entries,
/// which reference previously defined external items.
///
/// This match arm performs an `unsafe` dereference of a raw pointer
/// (`*const ComponentAlias`) in order to inspect the alias and compute
/// its section and external item kind.
///
/// ### Invariants
///
/// The following invariants guarantee that this operation is sound:
///
/// 1. **The raw pointer refers to a live IR node**
///
///    The `node` pointer stored in `ComponentItem::Alias` originates
///    from a `&ComponentAlias` reference obtained during the collect
///    phase. The encode plan does not outlive the IR that owns this
///    alias, and the assign phase executes while the IR is still alive.
///    Therefore, dereferencing `node` cannot observe freed memory.
///
/// 2. **The IR is immutable during assignment**
///
///    No mutable references to component IR nodes exist during the
///    assign phase. All IR structures are treated as read-only while
///    indices are being assigned. This ensures that dereferencing a
///    `*const ComponentAlias` does not violate Rust’s aliasing rules.
///
/// 3. **The pointer has the correct provenance and type**
///
///    The `node` pointer is never cast from an unrelated type. It is
///    created exclusively from a `&ComponentAlias` reference and stored
///    as a `*const ComponentAlias`. As a result, reinterpreting the
///    pointer as `&ComponentAlias` is well-defined.
///
/// 4. **Alias metadata is sufficient for index assignment**
///
///    The assign phase does not rely on alias indices being final or
///    globally unique at this point. It only uses alias metadata
///    (section and external item kind) to assign an actual index within
///    the appropriate component section. This metadata is stable and
///    independent of the eventual binary encoding order.
///
/// ### Why this happens in the assign phase
///
/// Alias entries may reference items defined earlier in the component,
/// and their indices depend on the final emission order. The assign
/// phase is responsible for:
///
/// - Determining the canonical order of component items
/// - Assigning section-local indices
/// - Building the mapping from original IR indices to encoded indices
///
/// Dereferencing the alias node here is necessary to compute the
/// correct `ExternalItemKind` for index assignment.
///
/// ### Safety boundary
///
/// The `unsafe` block marks the point where the encoder relies on the
/// invariants above. As long as the encode plan does not outlive the IR
/// and the IR remains immutable during assignment, this dereference is
/// sound.
///
/// Any future change that allows IR nodes to be dropped, moved, or
/// mutably borrowed during the assign phase must re-evaluate this
/// safety argument.
///
/// ### Summary
///
/// - The alias pointer always refers to a live, immutable IR node
/// - The pointer has correct type provenance
/// - The assign phase only performs read-only inspection
///
/// Therefore, dereferencing `*const ComponentAlias` during index
/// assignment is safe.
pub(crate) fn assign_indices<'a>(plan: &mut ComponentPlan<'a>, indices: &mut IdxSpaces) {
    for item in &mut plan.items {
        match item {
            ComponentItem::Component {
                node,
                plan: subplan,
                indices: subindices,
                idx,
            } => unsafe {
                // Visit this component's internals
                subindices.reset_ids();
                assign_indices(subplan, subindices);

                let ptr: &Component = &**node;
                indices.assign_actual_id(&ptr.index_space_of(), &ComponentSection::Component, *idx);
            },
            ComponentItem::Module { node, idx } => unsafe {
                let ptr: &Module = &**node;
                indices.assign_actual_id(&ptr.index_space_of(), &ComponentSection::Module, *idx);
            },
            ComponentItem::CompType { node, idx } => unsafe {
                let ptr: &ComponentType = &**node;
                indices.assign_actual_id(
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentType,
                    *idx,
                );
            },
            ComponentItem::CompInst { node, idx } => unsafe {
                let ptr: &ComponentInstance = &**node;
                indices.assign_actual_id(
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentInstance,
                    *idx,
                );
            },
            ComponentItem::CanonicalFunc { node, idx } => unsafe {
                let ptr: &CanonicalFunction = &**node;
                indices.assign_actual_id(&ptr.index_space_of(), &ComponentSection::Canon, *idx);
            },
            ComponentItem::Alias { node, idx } => unsafe {
                let ptr: &ComponentAlias = &**node;
                indices.assign_actual_id(&ptr.index_space_of(), &ComponentSection::Alias, *idx);
            },
            ComponentItem::Import { node, idx } => unsafe {
                let ptr: &ComponentImport = &**node;
                indices.assign_actual_id(
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentImport,
                    *idx,
                );
            },
            ComponentItem::CoreType { node, idx } => unsafe {
                let ptr: &CoreType = &**node;
                // let is_module = matches!(ptr, CoreType::Module(_));
                // if is_module {
                //     indices.enter_scope();
                // }
                indices.assign_actual_id(&ptr.index_space_of(), &ComponentSection::CoreType, *idx);

                // if is_module {
                //     indices.exit_scope();
                // }
            },
            ComponentItem::Inst { node, idx } => unsafe {
                let ptr: &Instance = &**node;
                indices.assign_actual_id(
                    &ptr.index_space_of(),
                    &ComponentSection::CoreInstance,
                    *idx,
                );
            },
            ComponentItem::Export { .. } => {
                // NA: exports don't get IDs
            }
            ComponentItem::Start { .. } => {
                // NA: Start sections don't get IDs
            }
            ComponentItem::CustomSection { .. } => {
                // NA: Custom sections don't get IDs
            }
        }
    }
}
