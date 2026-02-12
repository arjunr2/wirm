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
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::internal::VisitCtxInner;
use crate::ir::types::CustomSection;

pub fn traverse_component<V: ComponentVisitor>(
    component: &Component,
    visitor: &mut V,
) {
    let mut ctx = VisitCtx::new(component);
    traverse(component, visitor, &mut ctx);
}

fn traverse<'a, V: ComponentVisitor>(
    component: &'a Component,
    visitor: &mut V,
    ctx: &mut VisitCtx,
) {
    ctx.inner.push_component(component);
    visitor.enter_component(ctx, component);

    for (num, section) in component.sections.iter() {
        let start_idx = ctx.inner.visit_section(section, *num as usize);

        match section {
            ComponentSection::Component => {
                debug_assert!(start_idx + *num as usize <= component.components.len());

                for i in 0..*num {
                    let idx = start_idx + i as usize;
                    let subcomponent = &component.components[idx];
                    traverse(subcomponent, visitor, ctx);
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
                    visitor.visit_module(ctx, item);
                    ctx.inner.maybe_exit_scope(item);
                }
            }

            ComponentSection::ComponentType => visit_boxed_vec(
                &component.component_types.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, ty| {
                    visitor.visit_comp_type(ctx, ty);
                }
            ),
            ComponentSection::ComponentInstance => visit_vec(
                &component.component_instance[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, inst| {
                    visitor.visit_comp_instance(ctx, inst);
                }
            ),
            ComponentSection::Canon => visit_vec(
                &component.canons.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, canon| {
                    visitor.visit_canon(ctx, canon);
                }
            ),
            ComponentSection::Alias => visit_vec(
                &component.alias.items[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, alias| {
                    visitor.visit_alias(ctx, alias);
                }
            ),
            ComponentSection::ComponentImport => visit_vec(
                &component.imports[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, imp| {
                    visitor.visit_comp_import(ctx, imp);
                }
            ),
            ComponentSection::ComponentExport => visit_vec(
                &component.exports[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, exp| {
                    visitor.visit_comp_export(ctx, exp);
                }
            ),

            ComponentSection::CoreType => visit_boxed_vec(
                &component.core_types[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, ty| {
                    visitor.visit_core_type(ctx, ty);
                }
            ),
            ComponentSection::CoreInstance => visit_vec(
                &component.instances[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, inst| {
                    visitor.visit_core_instance(ctx, inst);
                }
            ),

            ComponentSection::CustomSection => visit_vec(
                &component.custom_sections.custom_sections[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, sect| {
                    visitor.visit_custom_section(ctx, sect);
                }
            ),
            ComponentSection::ComponentStartSection => visit_vec(
                &component.start_section[start_idx..start_idx+ *num as usize],
                ctx,
                visitor,
                |visitor, ctx, start| {
                    visitor.visit_start_section(ctx, start);
                }
            ),
        }
    }

    visitor.exit_component(ctx, component);
    ctx.inner.pop_component();
}

fn visit_vec<'a, T: GetScopeKind>(
    slice: &'a [T],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, &T)
) {
    for item in slice {
        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

fn visit_boxed_vec<'a, T: GetScopeKind>(
    slice: &'a [Box<T>],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, &T)
) {
    for item in slice {
        let item = item.as_ref();

        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, item);
        ctx.inner.maybe_exit_scope(item);
    }
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
    fn enter_component(&mut self, _cx: &VisitCtx, _component: &Component) {}
    fn exit_component(&mut self, _cx: &VisitCtx, _component: &Component) {}
    fn visit_module(&mut self, _cx: &VisitCtx, _module: &Module) {}

    // component-level items
    fn visit_comp_type(&mut self, _cx: &VisitCtx, _comp_type: &ComponentType) {}
    fn visit_comp_instance(&mut self, _cx: &VisitCtx, _instance: &ComponentInstance) {}
    fn visit_canon(&mut self, _cx: &VisitCtx, _instance: &CanonicalFunction) {}
    fn visit_alias(&mut self, _cx: &VisitCtx, _instance: &ComponentAlias) {}
    fn visit_comp_import(&mut self, _cx: &VisitCtx, _import: &ComponentImport) {}
    fn visit_comp_export(&mut self, _cx: &VisitCtx, _import: &ComponentExport) {}

    // core wasm items
    fn visit_core_type(&mut self, _cx: &VisitCtx, _ty: &CoreType) {}
    fn visit_core_instance(&mut self, _cx: &VisitCtx, _inst: &Instance) {}
    fn visit_custom_section(&mut self, _cx: &VisitCtx, _sect: &CustomSection) {}
    fn visit_start_section(&mut self, _cx: &VisitCtx, _start: &ComponentStartFunction) {}
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
pub struct VisitCtx {
    pub(crate) inner: VisitCtxInner,
}
impl VisitCtx {
    pub(crate) fn new(component: &Component) -> Self {
        Self {
            inner: VisitCtxInner::new(component),
        }
    }

    // TODO: Create a lot of node resolution functions here
    //       see examples below:
    // /// Resolves a type reference relative to the current traversal position.
    // ///
    // /// Returns the resolved [`ComponentType`] if the reference is valid.
    // pub fn resolve_type(
    //     &self,
    //     ty: &TypeRef,
    // ) -> Option<&'a ComponentType> {
    //     self.inner.resolve_type(ty)
    // }
    //
    // /// Resolves an exported item from an instance by name.
    // ///
    // /// Returns the resolved item if found.
    // pub fn resolve_instance_export(
    //     &self,
    //     instance: &'a Instance,
    //     name: &str,
    // ) -> Option<ResolvedItem<'a>> {
    //     self.inner.resolve_instance_export(instance, name)
    // }
    //
    // /// Returns the parent component if currently inside a nested component.
    // pub fn parent_component(&self) -> Option<&'a Component> {
    //     self.inner.parent_component()
    // }
}

/// A resolved component item.
///
/// This represents the semantic target of a reference after index
/// resolution.
pub enum ResolvedItem {
    // TODO
    // Component(&'a Component),
    // Module(&'a Module),
    // Type(&'a ComponentType),
    // Instance(&'a Instance),
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
    use crate::ir::component::idx_spaces::{Depth, IndexedRef, ScopeId, SpaceSubtype, StoreHandle};
    use crate::ir::component::scopes::{GetScopeKind, RegistryHandle};
    use crate::ir::component::section::ComponentSection;
    use crate::ir::id::ComponentId;

    pub struct VisitCtxInner {
        pub(crate) registry: RegistryHandle,
        pub(crate) component_stack: Vec<ComponentId>, // may not need
        pub(crate) scope_stack: ScopeStack,
        pub(crate) store: StoreHandle,
    }

    // =======================================
    // =========== SCOPE INTERNALS ===========
    // =======================================
    
    impl VisitCtxInner {
        pub fn new(root: &Component) -> Self {
            Self {
                registry: root.scope_registry.clone(),
                component_stack: Vec::new(),
                scope_stack: ScopeStack::new(root.space_id),
                store: root.index_store.clone(),
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
    }

    // ===============================================
    // =========== ID RESOLUTION INTERNALS ===========
    // ===============================================

    impl VisitCtxInner {
        fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
            let scope_id = self.scope_stack.space_at_depth(&r.depth);
            self.store
                .borrow()
                .scopes
                .get(&scope_id)
                .unwrap()
                .lookup_actual_id_or_panic(r)
        }

        fn index_from_assumed_id(&mut self, r: &IndexedRef) -> (SpaceSubtype, usize, Option<usize>) {
            let scope_id = self.scope_stack.space_at_depth(&r.depth);
            self.store
                .borrow_mut()
                .scopes
                .get_mut(&scope_id)
                .unwrap()
                .index_from_assumed_id(r)
        }
    }

    // =================================================
    // =========== NODE RESOLUTION INTERNALS ===========
    // =================================================

    impl VisitCtxInner {
        // TODO: Write resolution helpers here (see below for examples)
        // pub(crate) fn resolve_type(
        //     &self,
        //     ty: &TypeRef,
        // ) -> Option<&ComponentType> {
        //     let scope = self.scope_stack.last()?;
        //     self.registry.resolve_type(scope, ty)
        // }
        //
        // pub(crate) fn resolve_instance_export(
        //     &self,
        //     instance: &'a Instance,
        //     name: &str,
        // ) -> Option<ResolvedItem<'a>> {
        //     let scope = self.scope_stack.last()?;
        //     self.registry.resolve_instance_export(scope, instance, name)
        // }
        //
        // pub(crate) fn parent_component(&self) -> Option<&'a Component> {
        //     if self.component_stack.len() < 2 {
        //         return None;
        //     }
        //
        //     let parent_id = self.component_stack[self.component_stack.len() - 2];
        //     self.registry.component_by_id(parent_id)
        // }
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

