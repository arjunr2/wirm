//! ## Component Traversal and Resolution
//!
//! This crate provides structured traversal over WebAssembly components
//! via [`Component::visit`].
//!
//! During traversal, a [`VisitCtx`] is provided, allowing resolution of
//! type references, instance exports, and other indexed items relative
//! to the current component scope.
//!
//! This allows tools such as visualizers, analyzers, and documentation
//! generators to inspect component structure without reimplementing
//! index tracking logic.
//!
//! Internal index-space and scope mechanics are intentionally not exposed.
//! Consumers interact only with semantic resolution APIs.

use crate::ir::component::idx_spaces::{IndexSpaceOf, Space};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, CoreType, Instance,
};
use crate::ir::component::visitor::{ItemKind, VisitCtx};

/// Traverses a [`Component`] using the provided [`ComponentVisitor`].
///
/// This performs a structured, depth-aware, read-only walk of the component
/// tree. All items encountered during traversal are dispatched to the
/// corresponding visitor methods.
///
/// # Traversal Semantics
///
/// - Traversal is **read-only**.
/// - Structured nesting is guaranteed (`enter_component` / `exit_component`
///   are always properly paired).
/// - Traversal order is stable, deterministic, and guaranteed to
///   match original parse order.
///
/// The root component is visited first. Its `id` will be `None`.
///
/// # Intended Use Cases
///
/// - Static analysis
/// - Cross-reference extraction
/// - Graph construction
/// - Validation passes
/// - Visualization tooling
///
/// This API is not intended for mutation or transformation of the component.
pub fn traverse_component<'a, V: HackableVisitor<'a>>(component: &'a Component<'a>, visitor: &mut V) {
    let mut ctx = VisitCtx::new(component);
    traverse(component, None, visitor, &mut ctx);
}

/// A structured, read-only visitor over a [`Component`] tree.
///
/// All methods have default no-op implementations. Override only the
/// callbacks relevant to your use case.
///
/// # Guarantees
///
/// - `enter_component` and `exit_component` are always properly paired.
/// - Nested components are visited in a well-structured manner.
/// - IDs are resolved and stable within a single traversal.
///
/// # ID Semantics
///
/// - `id: None` is used only for the root component.
/// - All other items receive a resolved `u32` ID corresponding to their
///   index within the appropriate namespace at that depth.
/// - For items that may belong to multiple namespaces (e.g. imports,
///   exports, aliases, canonical functions), the `ItemKind` parameter
///   indicates the resolved kind of the item.
///
/// # Mutation
///
/// This visitor is strictly read-only. Implementations must not mutate
/// the underlying component structure.
pub trait HackableVisitor<'a> {
    /// Invoked when entering the root (outermost) component.
    /// Note that `enter_component` will NOT be called for root!
    /// This allows special consideration for the root without having
    /// to wrap the `id` parameter of `enter_component` with Option
    /// (root components have no IDs)
    ///
    /// This is the earliest hook available for a component.
    fn enter_root_component(&mut self, _cx: &mut VisitCtx<'a>, _component: &Component<'a>) {}
    /// Invoked after all items within the root component have been visited.
    ///
    /// Always paired with a prior `enter_root_component` call.
    fn exit_root_component(&mut self, _cx: &mut VisitCtx<'a>, _component: &Component<'a>) {}

    /// Invoked when entering an inner component of the root.
    fn enter_component(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked after all items within an inner component have been visited.
    ///
    /// Always paired with a prior `enter_component` call.
    fn exit_component(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked for each core WebAssembly module defined in the component.
    fn visit_module(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _module: &Module<'a>) {}

    // ------------------------
    // Component-level items
    // ------------------------

    /// Invoked for each component type definition.
    fn visit_comp_type(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _comp_type: &ComponentType<'a>) {}
    /// Invoked for each component instance.
    fn visit_comp_instance(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _instance: &ComponentInstance<'a>) {}

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------

    /// Invoked for canonical functions.
    ///
    /// The `kind` parameter indicates the resolved namespace of this item.
    fn visit_canon(
        &mut self,
        _cx: &mut VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _canon: &CanonicalFunction,
    ) {
    }
    /// Invoked for component aliases.
    ///
    /// The `kind` parameter indicates the resolved target namespace
    /// referenced by the alias.
    fn visit_alias(&mut self, _cx: &mut VisitCtx<'a>, _kind: ItemKind, _id: u32, _alias: &ComponentAlias<'a>) {}
    /// Invoked for component imports.
    ///
    /// The `kind` parameter identifies the imported item category
    /// (e.g. type, function, instance).
    fn visit_comp_import(
        &mut self,
        _cx: &mut VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _import: &ComponentImport<'a>,
    ) {
    }
    /// Invoked for component exports.
    ///
    /// The `kind` parameter identifies the exported item category.
    fn visit_comp_export(
        &mut self,
        _cx: &mut VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _export: &ComponentExport<'a>,
    ) {
    }

    // ------------------------
    // Core WebAssembly items
    // ------------------------

    /// Invoked for each core WebAssembly type.
    fn visit_core_type(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _ty: &CoreType<'a>) {}
    /// Invoked for each core WebAssembly instance.
    fn visit_core_instance(&mut self, _cx: &mut VisitCtx<'a>, _id: u32, _inst: &Instance<'a>) {}

    // ------------------------
    // Sections
    // ------------------------

    /// Invoked for each custom section encountered during traversal.
    ///
    /// Custom sections are visited in traversal order and are not
    /// associated with structured enter/exit pairing.
    fn visit_custom_section(&mut self, _cx: &mut VisitCtx<'a>, _sect: &CustomSection<'a>) {}
    /// Invoked if the component defines a start function.
    fn visit_start_section(&mut self, _cx: &mut VisitCtx<'a>, _start: &ComponentStartFunction) {}
}

fn traverse<'a, V: HackableVisitor<'a>>(
    component: &'a Component<'a>,
    comp_idx: Option<usize>,
    visitor: &mut V,
    ctx: &mut VisitCtx<'a>,
) {
    ctx.inner.push_component(component);
    let comp_id = if let Some(idx) = comp_idx {
        Some(
            ctx.inner
                .lookup_id_for(&Space::Comp, &ComponentSection::Component, idx),
        )
    } else {
        None
    };
    if let Some(id) = comp_id {
        visitor.enter_component(ctx, id, component);
    } else {
        visitor.enter_root_component(ctx, component);
    }

    for (num, section) in component.sections.iter() {
        let start_idx = ctx.inner.visit_section(section, *num as usize);

        match section {
            ComponentSection::Component => {
                debug_assert!(start_idx + *num as usize <= component.components.len());

                for i in 0..*num {
                    let idx = start_idx + i as usize;
                    let subcomponent = &component.components[idx];
                    traverse(subcomponent, Some(idx), visitor, ctx);
                }
            }
            ComponentSection::Module => {
                let start = start_idx;
                let num = *num as usize;
                let all = component.modules.as_vec();
                assert!(start + num <= all.len(), "{start} + {num} > {}", all.len());
                for i in 0..num {
                    let idx = start + i;
                    let item = &all[idx];

                    ctx.inner.maybe_enter_scope(item);
                    visitor.visit_module(
                        ctx,
                        ctx.inner
                            .lookup_id_for(&Space::CoreModule, &ComponentSection::Module, idx),
                        item,
                    );
                    ctx.inner.maybe_exit_scope(item);
                }
            }

            ComponentSection::ComponentType => visit_boxed_vec(
                &component.component_types.items[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, ty| {
                    visitor.visit_comp_type(
                        ctx,
                        ctx.inner.lookup_id_for(
                            &Space::CompType,
                            &ComponentSection::ComponentType,
                            idx,
                        ),
                        ty,
                    );
                },
            ),
            ComponentSection::ComponentInstance => visit_vec(
                &component.component_instance[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, inst| {
                    visitor.visit_comp_instance(
                        ctx,
                        ctx.inner.lookup_id_for(
                            &Space::CompInst,
                            &ComponentSection::ComponentInstance,
                            idx,
                        ),
                        inst,
                    );
                },
            ),
            ComponentSection::Canon => visit_vec(
                &component.canons.items[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, canon| {
                    let space = canon.index_space_of();
                    visitor.visit_canon(
                        ctx,
                        space.into(),
                        ctx.inner
                            .lookup_id_for(&space, &ComponentSection::Canon, idx),
                        canon,
                    );
                },
            ),
            ComponentSection::Alias => visit_vec(
                &component.alias.items[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, alias| {
                    let space = alias.index_space_of();
                    visitor.visit_alias(
                        ctx,
                        space.into(),
                        ctx.inner
                            .lookup_id_for(&space, &ComponentSection::Alias, idx),
                        alias,
                    );
                    // visitor.visit_alias(ctx, alias);
                },
            ),
            ComponentSection::ComponentImport => visit_vec(
                &component.imports[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, imp| {
                    let space = imp.index_space_of();
                    visitor.visit_comp_import(
                        ctx,
                        space.into(),
                        ctx.inner
                            .lookup_id_for(&space, &ComponentSection::ComponentImport, idx),
                        imp,
                    );
                },
            ),
            ComponentSection::ComponentExport => visit_vec(
                &component.exports[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, exp| {
                    let space = exp.index_space_of();
                    visitor.visit_comp_export(
                        ctx,
                        space.into(),
                        ctx.inner
                            .lookup_id_for(&space, &ComponentSection::ComponentExport, idx),
                        exp,
                    );
                },
            ),

            ComponentSection::CoreType => visit_boxed_vec(
                &component.core_types[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, ty| {
                    visitor.visit_core_type(
                        ctx,
                        ctx.inner
                            .lookup_id_for(&Space::CoreType, &ComponentSection::CoreType, idx),
                        ty,
                    );
                },
            ),
            ComponentSection::CoreInstance => visit_vec(
                &component.instances[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, inst| {
                    visitor.visit_core_instance(
                        ctx,
                        ctx.inner.lookup_id_for(
                            &Space::CoreInst,
                            &ComponentSection::CoreInstance,
                            idx,
                        ),
                        inst,
                    );
                },
            ),

            ComponentSection::CustomSection => visit_vec(
                &component.custom_sections.custom_sections[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, sect| {
                    visitor.visit_custom_section(ctx, sect);
                },
            ),
            ComponentSection::ComponentStartSection => visit_vec(
                &component.start_section[start_idx..start_idx + *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, start| {
                    visitor.visit_start_section(ctx, start);
                },
            ),
        }
    }

    if let Some(id) = comp_id {
        visitor.exit_component(ctx, id, component);
        ctx.inner.pop_component();
    } else {
        visitor.exit_root_component(ctx, component);
    }
}

fn visit_vec<'a, V, T>(
    slice: &'a [T],
    ctx: &mut VisitCtx<'a>,
    visitor: &mut V,
    start_idx: usize,
    visit: fn(&mut V, &mut VisitCtx<'a>, usize, &T),
)
where
    V: HackableVisitor<'a>,
    T: GetScopeKind,
{
    for (i, item) in slice.iter().enumerate() {
        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx + i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

fn visit_boxed_vec<'a, V, T>(
    slice: &'a [Box<T>],
    ctx: &mut VisitCtx<'a>,
    visitor: &mut V,
    start_idx: usize,
    visit: fn(&mut V, &mut VisitCtx<'a>, usize, &T),
)
where
    V: HackableVisitor<'a>,
    T: GetScopeKind,
{
    for (i, item) in slice.iter().enumerate() {
        let item = item.as_ref();

        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx + i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}
