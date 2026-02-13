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

use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, CoreType, Instance};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::Space;
use crate::ir::component::refs::{IndexedRef, RefKind};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::internal::VisitCtxInner;
use crate::ir::types::CustomSection;

pub fn traverse_component<V: ComponentVisitor>(
    component: &Component,
    visitor: &mut V,
) {
    let mut ctx = VisitCtx::new(component);
    traverse(component, None, visitor, &mut ctx);
}

/// A structured, read-only visitor over a [`Component`] tree.
///
/// All methods have default no-op implementations. Override only the
/// callbacks you are interested in.
///
/// Traversal order is stable but not guaranteed to reflect the original
/// parse order exactly. Consumers should not depend on ordering semantics
/// beyond structured nesting (enter/exit pairing).
pub trait ComponentVisitor {
    /// The ID is None when we're visiting the ROOT component
    fn enter_component(&mut self, _cx: &VisitCtx, _id: Option<u32>, _component: &Component) {}
    /// The ID is None when we're visiting the ROOT component
    fn exit_component(&mut self, _cx: &VisitCtx, _id: Option<u32>, _component: &Component) {}
    fn visit_module(&mut self, _cx: &VisitCtx, _id: u32, _module: &Module) {}

    // component-level items
    fn visit_comp_type(&mut self, _cx: &VisitCtx, _id: u32, _comp_type: &ComponentType) {}
    fn visit_comp_instance(&mut self, _cx: &VisitCtx, _id: u32, _instance: &ComponentInstance) {}

    // the below items must RESOLVE IDs as they can be of several different variations
    fn visit_canon(&mut self, _cx: &VisitCtx, _canon: &CanonicalFunction) {}
    fn visit_alias(&mut self, _cx: &VisitCtx, _alias: &ComponentAlias) {}
    fn visit_comp_import(&mut self, _cx: &VisitCtx, _import: &ComponentImport) {}
    fn visit_comp_export(&mut self, _cx: &VisitCtx, _export: &ComponentExport) {}

    // core wasm items
    fn visit_core_type(&mut self, _cx: &VisitCtx, _id: u32, _ty: &CoreType) {}
    fn visit_core_instance(&mut self, _cx: &VisitCtx, _id: u32, _inst: &Instance) {}
    fn visit_custom_section(&mut self, _cx: &VisitCtx, _sect: &CustomSection) {}
    fn visit_start_section(&mut self, _cx: &VisitCtx, _start: &ComponentStartFunction) {}
}

fn traverse<'a, V: ComponentVisitor>(
    component: &'a Component,
    comp_idx: Option<usize>,
    visitor: &mut V,
    ctx: &mut VisitCtx,
) {
    ctx.inner.push_component(component);
    let id = if let Some(idx) = comp_idx {
        Some(ctx.inner.lookup_id_for(&Space::Comp, &ComponentSection::Component, idx))
    } else {
        None
    };
    visitor.enter_component(ctx, id, component);

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
                    visitor.visit_module(ctx, ctx.inner.lookup_id_for(&Space::CoreModule, &ComponentSection::Module, idx), item);
                    ctx.inner.maybe_exit_scope(item);
                }
            }

            ComponentSection::ComponentType => visit_boxed_vec(
                &component.component_types.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, ty| {
                    visitor.visit_comp_type(ctx, ctx.inner.lookup_id_for(&Space::CompType, &ComponentSection::ComponentType, idx), ty);
                }
            ),
            ComponentSection::ComponentInstance => visit_vec(
                &component.component_instance[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, inst| {
                    visitor.visit_comp_instance(ctx, ctx.inner.lookup_id_for(&Space::CompInst, &ComponentSection::ComponentInstance, idx), inst);
                }
            ),
            ComponentSection::Canon => visit_vec(
                &component.canons.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, canon| {
                    visitor.visit_canon(ctx, canon);
                }
            ),
            ComponentSection::Alias => visit_vec(
                &component.alias.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, alias| {
                    visitor.visit_alias(ctx, alias);
                }
            ),
            ComponentSection::ComponentImport => visit_vec(
                &component.imports[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, imp| {
                    visitor.visit_comp_import(ctx, imp);
                }
            ),
            ComponentSection::ComponentExport => visit_vec(
                &component.exports[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, _, exp| {
                    visitor.visit_comp_export(ctx, exp);
                }
            ),

            ComponentSection::CoreType => visit_boxed_vec(
                &component.core_types[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, ty| {
                    visitor.visit_core_type(ctx, ctx.inner.lookup_id_for(&Space::CoreType, &ComponentSection::CoreType, idx), ty);
                }
            ),
            ComponentSection::CoreInstance => visit_vec(
                &component.instances[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, inst| {
                    visitor.visit_core_instance(ctx, ctx.inner.lookup_id_for(&Space::CoreInst, &ComponentSection::CoreInstance, idx), inst);
                }
            ),

            ComponentSection::CustomSection => visit_vec(
                &component.custom_sections.custom_sections[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, sect| {
                    visitor.visit_custom_section(ctx, sect);
                }
            ),
            ComponentSection::ComponentStartSection => visit_vec(
                &component.start_section[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                start_idx,
                |visitor, ctx, idx, start| {
                    visitor.visit_start_section(ctx, start);
                }
            ),
        }
    }

    visitor.exit_component(ctx, id, component);
    ctx.inner.pop_component();
}

fn visit_vec<'a, T: GetScopeKind>(
    slice: &'a [T],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    start_idx: usize,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, usize, &T)
) {
    for (i, item) in slice.iter().enumerate() {
        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx+ i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

fn visit_boxed_vec<'a, T: GetScopeKind>(
    slice: &'a [Box<T>],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    start_idx: usize,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, usize, &T)
) {
    for (i, item) in slice.iter().enumerate() {
        let item = item.as_ref();

        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx+ i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

/// Context provided during component traversal.
///
/// `VisitCtx` allows resolution of references (such as type indices or
/// instance exports) relative to the current traversal position.
///
/// The context:
///
/// - Tracks nested component boundaries
/// - Tracks nested index scopes
/// - Resolves `(outer ...)` references correctly
///
/// This type is opaque and cannot be constructed by users. It is only
/// available during traversal via [`Component::visit`].
///
/// All resolution operations are read-only and reflect the semantic
/// structure of the component, not its internal storage layout.
pub struct VisitCtx<'a> {
    pub(crate) inner: VisitCtxInner<'a>,
}
impl<'a> VisitCtx<'a> {
    pub(crate) fn new(component: &'a Component<'a>) -> Self {
        Self {
            inner: VisitCtxInner::new(component),
        }
    }
    pub fn resolve(&self, ref_: &IndexedRef) -> ResolvedItem {
        self.inner.resolve(ref_)
    }
    pub fn resolve_all(&self, refs: &Vec<RefKind>) -> Vec<ResolvedItem> {
        self.inner.resolve_all(refs)
    }
    pub fn lookup_comp_inst_name(&self, id: u32) -> Option<String> {
        self.inner.lookup_comp_inst_name(id)
    }
}

/// A resolved component item.
///
/// This represents the semantic target of a reference after index
/// resolution.
pub enum ResolvedItem<'a, 'b> {
    Component(u32, &'a Component<'b>),      // (ID, ir-node)
    Module(u32, &'a Module<'b>),

    Func(u32, &'a CanonicalFunction),
    CompType(u32, &'a ComponentType<'b>),
    CompInst(u32, &'a ComponentInstance<'b>),
    CoreInst(u32, &'a Instance<'b>),
    CoreType(u32, &'a CoreType<'b>),

    Alias(&'a ComponentAlias<'b>),
    Import(&'a ComponentImport<'b>),
    Export(&'a ComponentExport<'b>),

    // Value(&'a Module),
    // Memory(&'a Module),
    // Table(&'a Module),
    // Global(&'a Module),
    // Tag(&'a Module),
}

pub(crate) mod internal {
    //! Internal traversal and resolution machinery.
    //!
    //! This module mirrors the encode traversal logic but operates in a
    //! read-only mode. It maintains:
    //!
    //! - A stack of component identities
    //! - A stack of active index scopes
    //! - A reference to the scope registry
    //!
    //! It is intentionally not exposed publicly to avoid leaking implementation
    //! details such as pointer identity or scope IDs.
    //!
    //! # Safety and Invariants
    //!
    //! This traversal logic relies on the following invariants:
    //!
    //! - Component IDs are stable for the lifetime of the IR.
    //! - Scoped IR nodes are stored in stable allocations.
    //! - The scope registry is fully populated before traversal begins.
    //! - No mutation of the component occurs during traversal.
    //!
    //! These guarantees allow resolution to rely on structural identity
    //! without exposing internal identity mechanisms publicly.

    use crate::Component;
    use crate::ir::component::idx_spaces::{ScopeId, Space, SpaceSubtype, StoreHandle};
    use crate::ir::component::refs::{Depth, IndexedRef, RefKind};
    use crate::ir::component::scopes::{build_component_store, ComponentStore, GetScopeKind, RegistryHandle};
    use crate::ir::component::section::ComponentSection;
    use crate::ir::component::visitor::ResolvedItem;
    use crate::ir::id::ComponentId;

    pub struct VisitCtxInner<'a> {
        pub(crate) registry: RegistryHandle,
        pub(crate) component_stack: Vec<ComponentId>, // may not need
        pub(crate) scope_stack: ScopeStack,
        pub(crate) store: StoreHandle,
        pub(crate) comp_store: ComponentStore<'a>,
    }

    // =======================================
    // =========== SCOPE INTERNALS ===========
    // =======================================

    impl<'a> VisitCtxInner<'a> {
        pub fn new(root: &'a Component<'a>) -> Self {
            let comp_store = build_component_store(root);
            Self {
                registry: root.scope_registry.clone(),
                component_stack: Vec::new(),
                scope_stack: ScopeStack::new(root.space_id),
                store: root.index_store.clone(),
                comp_store
            }
        }

        pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
            let mut store = self.store.borrow_mut();
            let indices = {
                store
                    .scopes
                    .get_mut(&self.scope_stack.curr_space_id())
                    .unwrap()
            };
            indices.visit_section(section, num)
        }

        pub fn push_component(&mut self, component: &Component) {
            let id = component.id;
            self.component_stack.push(id);
            self.enter_comp_scope(id);
        }

        pub fn pop_component(&mut self) {
            let id = self.component_stack.pop().unwrap();
            self.exit_comp_scope(id);
        }

        pub fn maybe_enter_scope<T: GetScopeKind>(&mut self, node: &T) {
            if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
                self.scope_stack.enter_space(scope_entry.space);
            }
        }

        pub fn maybe_exit_scope<T: GetScopeKind>(&mut self, node: &T) {
            if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
                // Exit the nested index space...should be equivalent to the ID
                // of the scope that was entered by this node
                let exited_from = self.scope_stack.exit_space();
                debug_assert_eq!(scope_entry.space, exited_from);
            }
        }

        fn enter_comp_scope(&mut self, comp_id: ComponentId) {
            let Some(scope_id) = self.registry.borrow().scope_of_comp(comp_id) else {
                panic!("no scope found for component {:?}", comp_id);
            };
            self.scope_stack.enter_space(scope_id);
        }

        fn exit_comp_scope(&mut self, comp_id: ComponentId) {
            let Some(scope_id) = self.registry.borrow().scope_of_comp(comp_id) else {
                panic!("no scope found for component {:?}", comp_id);
            };
            let exited_from = self.scope_stack.exit_space();
            debug_assert_eq!(scope_id, exited_from);
        }

        fn comp_at(&self, depth: Depth) -> &ComponentId {
            self.component_stack
                .get(self.component_stack.len() - depth.val() as usize - 1)
                .unwrap_or_else(|| {
                    panic!(
                        "couldn't find component at depth {}; this is the current component stack: {:?}",
                        depth.val(),
                        self.component_stack
                    )
                })
        }
    }

    // ===============================================
    // =========== ID RESOLUTION INTERNALS ===========
    // ===============================================

    impl VisitCtxInner<'_> {
        pub(crate) fn lookup_id_for(&self, space: &Space, section: &ComponentSection, vec_idx: usize) -> u32 {
            let scope_id = self.scope_stack.curr_space_id();
            self.store
                .borrow()
                .scopes
                .get(&scope_id)
                .unwrap()
                .lookup_assumed_id(space, section, vec_idx) as u32
        }

        fn index_from_assumed_id(&self, r: &IndexedRef) -> (SpaceSubtype, usize, Option<usize>) {
            let scope_id = self.scope_stack.space_at_depth(&r.depth);
            self.store
                .borrow()
                .scopes
                .get(&scope_id)
                .unwrap()
                .index_from_assumed_id_no_cache(r)
        }
    }

    // =================================================
    // =========== NODE RESOLUTION INTERNALS ===========
    // =================================================

    impl VisitCtxInner<'_> {
        pub fn lookup_comp_inst_name(&self, id: u32) -> Option<String> {
            todo!()
        }

        pub fn resolve_all(&self, refs: &Vec<RefKind>) -> Vec<ResolvedItem> {
            let mut items = vec![];
            for r in refs.iter() {
                items.push(self.resolve(&r.ref_));
            }

            items
        }

        pub fn resolve(&self, r: &IndexedRef) -> ResolvedItem {
            let (vec, idx, subidx) = self.index_from_assumed_id(r);
            if r.space != Space::CoreType {
                assert!(
                    subidx.is_none(),
                    "only core types (with rec groups) should ever have subvec indices!"
                );
            }

            let comp_id = self.comp_at(r.depth);
            let referenced_comp = self.comp_store.get(comp_id);

            let space = r.space;
            match vec {
                SpaceSubtype::Main => match space {
                    Space::Comp => ResolvedItem::Component(
                        r.index,
                        &referenced_comp.components[idx]
                    ),
                    Space::CompType => ResolvedItem::CompType(
                        r.index,
                        &referenced_comp.component_types.items[idx]
                    ),
                    Space::CompInst => ResolvedItem::CompInst(
                        r.index,
                        &referenced_comp.component_instance[idx]
                    ),
                    Space::CoreInst => ResolvedItem::CoreInst(
                        r.index,
                        &referenced_comp.instances[idx]
                    ),
                    Space::CoreModule => ResolvedItem::Module(
                        r.index,
                        &referenced_comp.modules[idx]
                    ),
                    Space::CoreType => ResolvedItem::CoreType(
                        r.index,
                        &referenced_comp.core_types[idx]
                    ),
                    Space::CompFunc | Space::CoreFunc => ResolvedItem::Func(
                        r.index,
                        &referenced_comp.canons.items[idx]
                    ),
                    Space::CompVal
                    | Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                },
                SpaceSubtype::Export => ResolvedItem::Export(&referenced_comp.exports[idx]),
                SpaceSubtype::Import => ResolvedItem::Import(&referenced_comp.imports[idx]),
                SpaceSubtype::Alias => ResolvedItem::Alias(&referenced_comp.alias.items[idx])
            }
        }
    }

    #[derive(Clone)]
    pub(crate) struct ScopeStack {
        pub(crate) stack: Vec<ScopeId>,
    }
    impl ScopeStack {
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
}
