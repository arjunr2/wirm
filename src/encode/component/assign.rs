use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport};
use crate::encode::component::collect::{ComponentItem, ComponentPlan};

use crate::ir::component::idx_spaces::{ExternalItemKind, IdxSpaces};
use crate::ir::section::ComponentSection;

// Phase 2
/// # Safety of Alias Index Assignment
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
/// ## Invariants
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
/// ## Why this happens in the assign phase
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
/// ## Safety boundary
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
/// ## Summary
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
            ComponentItem::Component{ plan: subplan, indices: subindices, idx, .. } => {
                // Visit this component's internals
                indices.reset_ids();
                assign_indices(subplan, subindices);

                indices.assign_actual_id(&ComponentSection::Component, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::Module { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::Module, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CompType { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::ComponentType, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CompInst { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::ComponentInstance, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CanonicalFunc { node, idx } => {
                unsafe {
                    let ptr: &CanonicalFunction = &**node;
                    indices.assign_actual_id(&ComponentSection::Canon, &ExternalItemKind::from(ptr), *idx);
                }
            }
            ComponentItem::Alias { node, idx } => {
                unsafe {
                    let ptr: &ComponentAlias = &**node;
                    indices.assign_actual_id(&ComponentSection::Alias, &ExternalItemKind::from(ptr), *idx);
                }
            }
            ComponentItem::Import { node, idx } => {
                unsafe {
                    let ptr: &ComponentImport = &**node;
                    indices.assign_actual_id(&ComponentSection::ComponentImport, &ExternalItemKind::from(&ptr.ty), *idx);
                }
            }
            ComponentItem::Export { node, idx } => {
                unsafe {
                    let ptr: &ComponentExport = &**node;
                    indices.assign_actual_id(&ComponentSection::ComponentExport, &ExternalItemKind::from(&ptr.ty), *idx);
                }
            }
            ComponentItem::CoreType { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::CoreType, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::Inst { idx, .. } => {
                indices.assign_actual_id(&ComponentSection::CoreInstance, &ExternalItemKind::NA, *idx);
            }
            ComponentItem::CustomSection { .. } => {
                // NA: Custom sections don't get IDs
            }
        }
    }
}
