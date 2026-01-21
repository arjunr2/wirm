use crate::encode::component::collect::{ComponentItem, ComponentPlan, SubItemPlan};
use crate::encode::component::EncodeCtx;
use crate::ir::component::idx_spaces::IndexSpaceOf;
use crate::ir::component::section::ComponentSection;
use crate::{assert_registered, Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration,
    ModuleTypeDeclaration,
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
pub(crate) fn assign_indices(plan: &mut ComponentPlan, ctx: &mut EncodeCtx) {
    for item in &mut plan.items {
        match item {
            ComponentItem::Component {
                node,
                plan: subplan,
                idx,
            } => unsafe {
                let ptr: &Component = &**node;

                // Visit this component's internals
                let scope_id = ctx.registry.borrow().scope_of_comp(ptr.id).unwrap();
                ctx.store.borrow_mut().reset_ids(&scope_id);
                ctx.space_stack.enter_space(scope_id);
                assign_indices(subplan, ctx);
                ctx.space_stack.exit_space();

                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::Component,
                    *idx,
                );
            },
            ComponentItem::Module { node, idx } => unsafe {
                let ptr: &Module = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::Module,
                    *idx,
                );
            },
            ComponentItem::CompType {
                node,
                idx,
                subitem_plan,
            } => unsafe {
                let ptr: &ComponentType = &**node;
                assignments_for_comp_ty(ptr, subitem_plan, ctx);

                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentType,
                    *idx,
                );
            },
            ComponentItem::CompInst { node, idx } => unsafe {
                let ptr: &ComponentInstance = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentInstance,
                    *idx,
                );
            },
            ComponentItem::CanonicalFunc { node, idx } => unsafe {
                let ptr: &CanonicalFunction = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::Canon,
                    *idx,
                );
            },
            ComponentItem::Alias { node, idx } => unsafe {
                let ptr: &ComponentAlias = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::Alias,
                    *idx,
                );
            },
            ComponentItem::Import { node, idx } => unsafe {
                let ptr: &ComponentImport = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentImport,
                    *idx,
                );
            },
            ComponentItem::CoreType {
                node,
                idx,
                subitem_plan,
            } => unsafe {
                let ptr: &CoreType = &**node;
                assignments_for_core_ty(ptr, *idx, subitem_plan, ctx);

                if matches!(ptr, CoreType::Module(_)) {
                    // only want to do this flat space assignment for a core type Module
                    ctx.store.borrow_mut().assign_actual_id(
                        &ctx.space_stack.curr_space_id(),
                        &ptr.index_space_of(),
                        &ComponentSection::CoreType,
                        *idx,
                    );
                }
            },
            ComponentItem::Inst { node, idx } => unsafe {
                let ptr: &Instance = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::CoreInstance,
                    *idx,
                );
            },
            ComponentItem::Export { node, idx } => unsafe {
                let ptr: &ComponentExport = &**node;
                ctx.store.borrow_mut().assign_actual_id(
                    &ctx.space_stack.curr_space_id(),
                    &ptr.index_space_of(),
                    &ComponentSection::ComponentExport,
                    *idx,
                );
            },
            ComponentItem::Start { .. } => {
                // NA: Start sections don't get IDs
            }
            ComponentItem::CustomSection { .. } => {
                // NA: Custom sections don't get IDs
            }
        }
    }
}

pub(crate) fn assignments_for_comp_ty(
    ty: &ComponentType,
    subitem_plan: &Option<SubItemPlan>,
    ctx: &mut EncodeCtx,
) -> ComponentSection {
    match ty {
        ComponentType::Component(decls) => {
            ctx.maybe_enter_scope(ty);
            assert_registered!(ctx.registry, ty);

            let section = ComponentSection::ComponentType;
            for (idx, subplan) in subitem_plan.as_ref().unwrap().order().iter() {
                let decl = &decls[*idx];
                assignments_for_comp_ty_comp_decl(*idx, subplan, decl, &section, ctx);
            }

            ctx.maybe_exit_scope(ty);
            section
        }
        ComponentType::Instance(decls) => {
            ctx.maybe_enter_scope(ty);
            assert_registered!(ctx.registry, ty);

            let section = ComponentSection::ComponentType;
            if let Some(subplan) = subitem_plan {
                for (idx, subplan) in subplan.order().iter() {
                    let decl = &decls[*idx];
                    assignments_for_comp_ty_inst_decl(*idx, subplan, decl, &section, ctx);
                }
            }

            ctx.maybe_exit_scope(ty);
            section
        }
        _ => ComponentSection::ComponentType,
    }
}

fn assignments_for_comp_ty_comp_decl(
    decl_idx: usize,
    subitem_plan: &Option<SubItemPlan>,
    decl: &ComponentTypeDeclaration,
    section: &ComponentSection,
    ctx: &mut EncodeCtx,
) {
    let space = decl.index_space_of();
    ctx.store.borrow_mut().assign_actual_id(
        &ctx.space_stack.curr_space_id(),
        &space,
        section,
        decl_idx,
    );

    match decl {
        ComponentTypeDeclaration::CoreType(ty) => {
            assignments_for_core_ty(ty, decl_idx, subitem_plan, ctx);
        }
        ComponentTypeDeclaration::Type(ty) => {
            assignments_for_comp_ty(ty, subitem_plan, ctx);
        }
        ComponentTypeDeclaration::Alias(_)
        | ComponentTypeDeclaration::Export { .. }
        | ComponentTypeDeclaration::Import(_) => {}
    }
}

fn assignments_for_comp_ty_inst_decl(
    decl_idx: usize,
    subitem_plan: &Option<SubItemPlan>,
    decl: &InstanceTypeDeclaration,
    section: &ComponentSection,
    ctx: &mut EncodeCtx,
) {
    let space = decl.index_space_of();
    ctx.store.borrow_mut().assign_actual_id(
        &ctx.space_stack.curr_space_id(),
        &space,
        section,
        decl_idx,
    );

    match decl {
        InstanceTypeDeclaration::CoreType(ty) => {
            assignments_for_core_ty(ty, decl_idx, subitem_plan, ctx);
        }
        InstanceTypeDeclaration::Type(ty) => {
            assignments_for_comp_ty(ty, subitem_plan, ctx);
        }
        InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => {}
    }
}

pub(crate) fn assignments_for_core_ty(
    ty: &CoreType,
    ty_idx: usize,
    subitem_plan: &Option<SubItemPlan>,
    ctx: &mut EncodeCtx,
) -> ComponentSection {
    let section = ComponentSection::CoreType;
    match ty {
        CoreType::Module(decls) => {
            ctx.maybe_enter_scope(ty);
            assert_registered!(ctx.registry, ty);

            for (idx, subplan) in subitem_plan.as_ref().unwrap().order().iter() {
                assert!(subplan.is_none());
                let decl = &decls[*idx];
                assignments_for_core_module_decl(*idx, decl, &section, ctx);
            }

            ctx.maybe_exit_scope(ty);
            section
        }
        CoreType::Rec(recgroup) => {
            for (subty_idx, subty) in recgroup.types().enumerate() {
                ctx.store.borrow_mut().assign_actual_id_with_subvec(
                    &ctx.space_stack.curr_space_id(),
                    &subty.index_space_of(),
                    &section,
                    ty_idx,
                    subty_idx,
                );
            }

            ComponentSection::CoreType
        }
    }
}

fn assignments_for_core_module_decl(
    decl_idx: usize,
    decl: &ModuleTypeDeclaration,
    section: &ComponentSection,
    ctx: &mut EncodeCtx,
) {
    let space = decl.index_space_of();
    ctx.store.borrow_mut().assign_actual_id(
        &ctx.space_stack.curr_space_id(),
        &space,
        section,
        decl_idx,
    );
}
