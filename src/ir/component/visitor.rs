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
use crate::ir::component::refs::{IndexedRef, RefKind};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::internal::VisitCtxInner;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, CoreType, Instance,
};

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
pub fn traverse_component<V: ComponentVisitor>(component: &Component, visitor: &mut V) {
    let mut ctx = VisitCtx::new(component);
    traverse(component, true, None, visitor, &mut ctx);
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
pub trait ComponentVisitor {
    /// Invoked when entering a component.
    ///
    /// The `id` will be:
    /// - `None` for the root component
    /// - `Some(id)` for nested components
    ///
    /// This is the earliest hook available for a component.
    fn enter_component(&mut self, _cx: &VisitCtx, _id: Option<u32>, _component: &Component) {}
    /// Invoked after all items within a component have been visited.
    ///
    /// Always paired with a prior `enter_component` call.
    fn exit_component(&mut self, _cx: &VisitCtx, _id: Option<u32>, _component: &Component) {}
    /// Invoked for each core WebAssembly module defined in the component.
    fn visit_module(&mut self, _cx: &VisitCtx, _id: u32, _module: &Module) {}

    // ------------------------
    // Component-level items
    // ------------------------

    /// Invoked for each component type definition.
    fn visit_comp_type(&mut self, _cx: &VisitCtx, _id: u32, _comp_type: &ComponentType) {}
    /// Invoked for each component instance.
    fn visit_comp_instance(&mut self, _cx: &VisitCtx, _id: u32, _instance: &ComponentInstance) {}

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------

    /// Invoked for canonical functions.
    ///
    /// The `kind` parameter indicates the resolved namespace of this item.
    fn visit_canon(
        &mut self,
        _cx: &VisitCtx,
        _kind: ItemKind,
        _id: u32,
        _canon: &CanonicalFunction,
    ) {
    }
    /// Invoked for component aliases.
    ///
    /// The `kind` parameter indicates the resolved target namespace
    /// referenced by the alias.
    fn visit_alias(&mut self, _cx: &VisitCtx, _kind: ItemKind, _id: u32, _alias: &ComponentAlias) {}
    /// Invoked for component imports.
    ///
    /// The `kind` parameter identifies the imported item category
    /// (e.g. type, function, instance).
    fn visit_comp_import(
        &mut self,
        _cx: &VisitCtx,
        _kind: ItemKind,
        _id: u32,
        _import: &ComponentImport,
    ) {
    }
    /// Invoked for component exports.
    ///
    /// The `kind` parameter identifies the exported item category.
    fn visit_comp_export(
        &mut self,
        _cx: &VisitCtx,
        _kind: ItemKind,
        _id: u32,
        _export: &ComponentExport,
    ) {
    }

    // ------------------------
    // Core WebAssembly items
    // ------------------------

    /// Invoked for each core WebAssembly type.
    fn visit_core_type(&mut self, _cx: &VisitCtx, _id: u32, _ty: &CoreType) {}
    /// Invoked for each core WebAssembly instance.
    fn visit_core_instance(&mut self, _cx: &VisitCtx, _id: u32, _inst: &Instance) {}

    // ------------------------
    // Sections
    // ------------------------

    /// Invoked for each custom section encountered during traversal.
    ///
    /// Custom sections are visited in traversal order and are not
    /// associated with structured enter/exit pairing.
    fn visit_custom_section(&mut self, _cx: &VisitCtx, _sect: &CustomSection) {}
    /// Invoked if the component defines a start function.
    fn visit_start_section(&mut self, _cx: &VisitCtx, _start: &ComponentStartFunction) {}
}

fn traverse<V: ComponentVisitor>(
    component: &Component,
    is_root: bool,
    comp_idx: Option<usize>,
    visitor: &mut V,
    ctx: &mut VisitCtx,
) {
    ctx.inner.push_component(component);
    let id = if let Some(idx) = comp_idx {
        Some(
            ctx.inner
                .lookup_id_for(&Space::Comp, &ComponentSection::Component, idx),
        )
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
                    traverse(subcomponent, false, Some(idx), visitor, ctx);
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

    visitor.exit_component(ctx, id, component);
    if !is_root {
        ctx.inner.pop_component();
    }
}

fn visit_vec<'a, T: GetScopeKind>(
    slice: &'a [T],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    start_idx: usize,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, usize, &T),
) {
    for (i, item) in slice.iter().enumerate() {
        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx + i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

fn visit_boxed_vec<'a, T: GetScopeKind>(
    slice: &'a [Box<T>],
    ctx: &mut VisitCtx,
    visitor: &mut dyn ComponentVisitor,
    start_idx: usize,
    visit: fn(&mut dyn ComponentVisitor, &mut VisitCtx, usize, &T),
) {
    for (i, item) in slice.iter().enumerate() {
        let item = item.as_ref();

        ctx.inner.maybe_enter_scope(item);
        visit(visitor, ctx, start_idx + i, item);
        ctx.inner.maybe_exit_scope(item);
    }
}

pub enum ItemKind {
    Comp,
    CompFunc,
    CompVal,
    CompType,
    CompInst,
    CoreInst,
    CoreModule,
    CoreType,
    CoreFunc,
    CoreMemory,
    CoreTable,
    CoreGlobal,
    CoreTag,
}
impl From<Space> for ItemKind {
    fn from(space: Space) -> Self {
        match space {
            Space::Comp => Self::Comp,
            Space::CompFunc => Self::CompFunc,
            Space::CompVal => Self::CompVal,
            Space::CompType => Self::CompType,
            Space::CompInst => Self::CompInst,
            Space::CoreInst => Self::CoreInst,
            Space::CoreModule => Self::CoreModule,
            Space::CoreType => Self::CoreType,
            Space::CoreFunc => Self::CoreFunc,
            Space::CoreMemory => Self::CoreMemory,
            Space::CoreTable => Self::CoreTable,
            Space::CoreGlobal => Self::CoreGlobal,
            Space::CoreTag => Self::CoreTag,
        }
    }
}

/// Context provided during component traversal.
///
/// `VisitCtx` allows resolution of referenced indices (such as type,
/// function, instance, or module indices) relative to the current
/// traversal position.
///
/// The context:
///
/// - Tracks nested component boundaries
/// - Tracks nested index scopes
/// - Correctly resolves `(outer ...)` references
/// - Resolves references across component and core index spaces
///
/// This type is opaque and cannot be constructed by users. It is only
/// available during traversal via [`traverse_component`].
///
/// All resolution operations are read-only and reflect the *semantic*
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
    /// Resolves a single [`IndexedRef`] into a fully resolved semantic item.
    ///
    /// This applies:
    ///
    /// - Depth resolution (`outer` / nested scopes)
    /// - Index space resolution
    /// - Component vs core namespace resolution
    ///
    /// The returned [`ResolvedItem`] represents the semantic target
    /// referenced by the index.
    pub fn resolve(&self, ref_: &IndexedRef) -> ResolvedItem<'_, '_> {
        self.inner.resolve(ref_)
    }
    /// Resolves a collection of [`RefKind`] values into their semantic targets.
    ///
    /// This is a convenience helper for bulk resolution when a node exposes
    /// multiple referenced indices.
    pub fn resolve_all(&self, refs: &[RefKind]) -> Vec<ResolvedItem<'_, '_>> {
        self.inner.resolve_all(refs)
    }
    /// Looks up the name (if any) of a component instance by its ID.
    ///
    /// Returns `None` if:
    /// - The instance has no name
    /// - The ID is not valid in the current context
    pub fn lookup_comp_inst_name(&self, id: u32) -> Option<&str> {
        self.inner.lookup_comp_inst_name(id)
    }
}

/// A resolved component item.
///
/// This represents the semantic target of a reference after index
/// resolution.
pub enum ResolvedItem<'a, 'b> {
    Component(u32, &'a Component<'b>), // (ID, ir-node)
    Module(u32, &'a Module<'b>),

    Func(u32, &'a CanonicalFunction),
    CompType(u32, &'a ComponentType<'b>),
    CompInst(u32, &'a ComponentInstance<'b>),
    CoreInst(u32, &'a Instance<'b>),
    CoreType(u32, &'a CoreType<'b>),

    Alias(u32, &'a ComponentAlias<'b>),
    Import(u32, &'a ComponentImport<'b>),
    Export(u32, &'a ComponentExport<'b>),
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

    use crate::ir::component::idx_spaces::{ScopeId, Space, SpaceSubtype, StoreHandle};
    use crate::ir::component::refs::{Depth, IndexedRef, RefKind};
    use crate::ir::component::scopes::{
        build_component_store, ComponentStore, GetScopeKind, RegistryHandle,
    };
    use crate::ir::component::section::ComponentSection;
    use crate::ir::component::visitor::{ResolvedItem, SectionTracker};
    use crate::ir::id::ComponentId;
    use crate::Component;

    pub struct VisitCtxInner<'a> {
        pub(crate) registry: RegistryHandle,
        pub(crate) component_stack: Vec<ComponentId>, // may not need
        pub(crate) scope_stack: ScopeStack,
        pub(crate) node_has_nested_scope: Vec<bool>,
        pub(crate) store: StoreHandle,
        pub(crate) comp_store: ComponentStore<'a>,
        section_tracker_stack: Vec<SectionTracker>,
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
                section_tracker_stack: Vec::new(),
                scope_stack: ScopeStack::new(),
                node_has_nested_scope: Vec::new(),
                store: root.index_store.clone(),
                comp_store,
            }
        }

        pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
            self.section_tracker_stack
                .last_mut()
                .unwrap()
                .visit_section(section, num)
            // let mut store = self.store.borrow_mut();
            // let indices = {
            //     store
            //         .scopes
            //         .get_mut(&self.scope_stack.curr_space_id())
            //         .unwrap()
            // };
            // indices.visit_section(section, num)
        }

        pub fn push_component(&mut self, component: &Component) {
            let id = component.id;
            self.component_stack.push(id);
            self.section_tracker_stack.push(SectionTracker::default());
            self.enter_comp_scope(id);
        }

        pub fn pop_component(&mut self) {
            let id = self.component_stack.pop().unwrap();
            self.section_tracker_stack.pop();
            self.exit_comp_scope(id);
        }
        pub fn curr_component(&self) -> &Component<'_> {
            let id = self.comp_at(Depth::default());
            self.comp_store.get(id)
        }

        pub fn maybe_enter_scope<T: GetScopeKind>(&mut self, node: &T) {
            let mut nested = false;
            if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
                nested = true;
                self.scope_stack.enter_space(scope_entry.space);
            }
            self.node_has_nested_scope.push(nested);
        }

        pub fn maybe_exit_scope<T: GetScopeKind>(&mut self, node: &T) {
            let nested = self.node_has_nested_scope.pop().unwrap();
            if let Some(scope_entry) = self.registry.borrow().scope_entry(node) {
                // Exit the nested index space...should be equivalent to the ID
                // of the scope that was entered by this node
                let exited_from = self.scope_stack.exit_space();
                debug_assert!(nested);
                debug_assert_eq!(scope_entry.space, exited_from);
            } else {
                debug_assert!(!nested);
            }
        }

        fn enter_comp_scope(&mut self, comp_id: ComponentId) {
            let Some(scope_id) = self.registry.borrow().scope_of_comp(comp_id) else {
                panic!("no scope found for component {:?}", comp_id);
            };
            self.node_has_nested_scope
                .push(!self.scope_stack.stack.is_empty());
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
        pub(crate) fn lookup_id_for(
            &self,
            space: &Space,
            section: &ComponentSection,
            vec_idx: usize,
        ) -> u32 {
            let nested = self.node_has_nested_scope.last().unwrap_or(&false);
            let scope_id = if *nested {
                self.scope_stack.space_at_depth(&Depth::parent())
            } else {
                self.scope_stack.curr_space_id()
            };
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
        pub fn lookup_comp_inst_name(&self, id: u32) -> Option<&str> {
            self.curr_component().instance_names.get(id)
        }

        pub fn resolve_all(&self, refs: &[RefKind]) -> Vec<ResolvedItem<'_, '_>> {
            let mut items = vec![];
            for r in refs.iter() {
                items.push(self.resolve(&r.ref_));
            }

            items
        }

        pub fn resolve(&self, r: &IndexedRef) -> ResolvedItem<'_, '_> {
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
                    Space::Comp => {
                        ResolvedItem::Component(r.index, &referenced_comp.components[idx])
                    }
                    Space::CompType => {
                        ResolvedItem::CompType(r.index, &referenced_comp.component_types.items[idx])
                    }
                    Space::CompInst => {
                        ResolvedItem::CompInst(r.index, &referenced_comp.component_instance[idx])
                    }
                    Space::CoreInst => {
                        ResolvedItem::CoreInst(r.index, &referenced_comp.instances[idx])
                    }
                    Space::CoreModule => {
                        ResolvedItem::Module(r.index, &referenced_comp.modules[idx])
                    }
                    Space::CoreType => {
                        ResolvedItem::CoreType(r.index, &referenced_comp.core_types[idx])
                    }
                    Space::CompFunc | Space::CoreFunc => {
                        ResolvedItem::Func(r.index, &referenced_comp.canons.items[idx])
                    }
                    Space::CompVal
                    | Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                },
                SpaceSubtype::Export => {
                    ResolvedItem::Export(r.index, &referenced_comp.exports[idx])
                }
                SpaceSubtype::Import => {
                    ResolvedItem::Import(r.index, &referenced_comp.imports[idx])
                }
                SpaceSubtype::Alias => {
                    ResolvedItem::Alias(r.index, &referenced_comp.alias.items[idx])
                }
            }
        }
    }

    #[derive(Clone)]
    pub(crate) struct ScopeStack {
        pub(crate) stack: Vec<ScopeId>,
    }
    impl ScopeStack {
        fn new() -> Self {
            Self { stack: vec![] }
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

// General trackers for indices of item vectors (used to track where i've been during visitation)
#[derive(Default)]
struct SectionTracker {
    last_processed_module: usize,
    last_processed_alias: usize,
    last_processed_core_ty: usize,
    last_processed_comp_ty: usize,
    last_processed_imp: usize,
    last_processed_exp: usize,
    last_processed_core_inst: usize,
    last_processed_comp_inst: usize,
    last_processed_canon: usize,
    last_processed_component: usize,
    last_processed_start: usize,
    last_processed_custom: usize,
}
impl SectionTracker {
    /// This function is used while traversing the component. This means that we
    /// should already know the space ID associated with the component section
    /// (if in visiting this next session we enter some inner index space).
    ///
    /// So, we use the associated space ID to return the inner index space. The
    /// calling function should use this return value to then context switch into
    /// this new index space. When we've finished visiting the section, swap back
    /// to the returned index space's `parent` (a field on the space).
    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        let tracker = match section {
            ComponentSection::Component => &mut self.last_processed_component,
            ComponentSection::Module => &mut self.last_processed_module,
            ComponentSection::Alias => &mut self.last_processed_alias,
            ComponentSection::CoreType => &mut self.last_processed_core_ty,
            ComponentSection::ComponentType => &mut self.last_processed_comp_ty,
            ComponentSection::ComponentImport => &mut self.last_processed_imp,
            ComponentSection::ComponentExport => &mut self.last_processed_exp,
            ComponentSection::CoreInstance => &mut self.last_processed_core_inst,
            ComponentSection::ComponentInstance => &mut self.last_processed_comp_inst,
            ComponentSection::Canon => &mut self.last_processed_canon,
            ComponentSection::CustomSection => &mut self.last_processed_custom,
            ComponentSection::ComponentStartSection => &mut self.last_processed_start,
        };

        let curr = *tracker;
        *tracker += num;
        curr
    }
}
