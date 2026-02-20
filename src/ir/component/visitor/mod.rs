use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, CoreType, Instance};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::Space;
use crate::ir::component::refs::{IndexedRef, RefKind};
use crate::ir::component::visitor::driver::{drive_event, VisitEvent};
use crate::ir::component::visitor::events_structural::get_structural_evts;
use crate::ir::component::visitor::events_topological::get_topological_evts;
use crate::ir::component::visitor::utils::VisitCtxInner;
use crate::ir::types::CustomSection;

mod driver;
mod events_structural;
pub(crate) mod utils;
mod events_topological;

/// Walk a [`Component`] using its *structural* (in-file) order.
///
/// This traversal visits items in the same order they appear in the
/// component’s section layout. Nested components are entered and exited
/// according to their lexical structure, and all visitor callbacks are
/// invoked in a manner consistent with the original encoding order.
///
/// # Semantics
///
/// - Preserves section order exactly as defined in the component.
/// - Nested components are visited depth-first.
/// - Scope management and ID resolution are handled internally.
/// - No reordering is performed to satisfy reference dependencies.
///
/// This is the most appropriate traversal for:
///
/// - Analysis passes
/// - Pretty-printing
/// - Validation-like inspections
/// - Any logic that expects lexical ordering
///
/// # Guarantees
///
/// - No forward-reference elimination is attempted.
/// - The visitor observes the same structural hierarchy as encoded.
/// - `enter_component` / `exit_component` callbacks are properly paired.
///
/// See also [`walk_topological`] for a dependency-ordered traversal.
pub fn walk_structural<'ir, V: ComponentVisitor<'ir>>(
    root: &'ir Component<'ir>,
    visitor: &mut V,
) {
    walk(get_structural_evts, root, visitor);
}

/// Walk a [`Component`] in *topological* (dependency) order.
///
/// This traversal reorders items such that definitions are visited
/// before any items that reference them. The resulting visitation
/// order contains no forward references, making it suitable for
/// encoding or transforming components that require dependency-safe
/// emission.
///
/// # Semantics
///
/// - Items are visited in a dependency-respecting order.
/// - Nested components are still entered/exited with correct scope
///   management.
/// - All visitor callbacks observe valid, already-declared references.
/// - Structural layout order is **not** preserved.
///
/// # When to Use
///
/// This traversal is intended for:
///
/// - Component encoding
/// - Instrumentation passes
/// - Lowering or rewriting IR where forward references are illegal
/// - Any pass that requires reference-safe emission order
///
/// # Guarantees
///
/// - No visitor callback observes an unresolved forward reference.
/// - Scope handling and ID resolution remain logically consistent.
/// - `enter_component` / `exit_component` callbacks are properly paired.
///
/// See also [`walk_structural`] for lexical-order traversal.
pub fn walk_topological<'ir, V: ComponentVisitor<'ir>>(
    root: &'ir Component<'ir>,
    visitor: &mut V,
) {
    walk(get_topological_evts, root, visitor);
}

fn walk<'ir, V: ComponentVisitor<'ir>>(
    get_evts: fn (&'ir Component<'ir>, Option<usize>, &mut VisitCtx<'ir>, &mut Vec<VisitEvent<'ir>>),
    root: &'ir Component<'ir>,
    visitor: &mut V,
) {
    let mut ctx = VisitCtx::new(root);
    let mut events = Vec::new();
    get_evts(root, None, &mut ctx, &mut events);

    for event in events {
        drive_event(event, visitor, &mut ctx);
    }
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
pub trait ComponentVisitor<'a> {
    /// Invoked when entering the outermost, root component to enable special handling.
    ///
    /// This is the earliest hook available for a component.
    fn enter_root_component(&mut self, _cx: &VisitCtx<'a>, _component: &Component<'a>) {}
    /// Invoked after all items within the root component have been visited.
    ///
    /// Always paired with a prior `enter_root_component` call.
    fn exit_root_component(&mut self, _cx: &VisitCtx<'a>, _component: &Component<'a>) {}
    /// Invoked when entering a subcomponent within the root.
    fn enter_component(&mut self, _cx: &VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked after all items within a subcomponent have been visited.
    ///
    /// Always paired with a prior `enter_component` call.
    fn exit_component(&mut self, _cx: &VisitCtx<'a>, _id: u32, _component: &Component<'a>) {}
    /// Invoked for each core WebAssembly module defined in the component.
    fn visit_module(&mut self, _cx: &VisitCtx<'a>, _id: u32, _module: &Module<'a>) {}

    // ------------------------
    // Component-level items
    // ------------------------

    /// Invoked for each component type definition.
    fn visit_comp_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _comp_type: &ComponentType<'a>) {}
    /// Invoked for each component instance.
    fn visit_comp_instance(&mut self, _cx: &VisitCtx<'a>, _id: u32, _instance: &ComponentInstance<'a>) {}

    // ------------------------------------------------
    // Items with multiple possible resolved namespaces
    // ------------------------------------------------

    /// Invoked for canonical functions.
    ///
    /// The `kind` parameter indicates the resolved namespace of this item.
    fn visit_canon(
        &mut self,
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _canon: &CanonicalFunction,
    ) {
    }
    /// Invoked for component aliases.
    ///
    /// The `kind` parameter indicates the resolved target namespace
    /// referenced by the alias.
    fn visit_alias(&mut self, _cx: &VisitCtx<'a>, _kind: ItemKind, _id: u32, _alias: &ComponentAlias<'a>) {}
    /// Invoked for component imports.
    ///
    /// The `kind` parameter identifies the imported item category
    /// (e.g. type, function, instance).
    fn visit_comp_import(
        &mut self,
        _cx: &VisitCtx<'a>,
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
        _cx: &VisitCtx<'a>,
        _kind: ItemKind,
        _id: u32,
        _export: &ComponentExport<'a>,
    ) {
    }

    // ------------------------
    // Core WebAssembly items
    // ------------------------

    /// Invoked for each core WebAssembly type.
    fn visit_core_type(&mut self, _cx: &VisitCtx<'a>, _id: u32, _ty: &CoreType<'a>) {}
    /// Invoked for each core WebAssembly instance.
    fn visit_core_instance(&mut self, _cx: &VisitCtx<'a>, _id: u32, _inst: &Instance<'a>) {}

    // ------------------------
    // Sections
    // ------------------------

    /// Invoked for each custom section encountered during traversal.
    ///
    /// Custom sections are visited in traversal order and are not
    /// associated with structured enter/exit pairing.
    fn visit_custom_section(&mut self, _cx: &VisitCtx<'a>, _sect: &CustomSection<'a>) {}
    /// Invoked if the component defines a start function.
    fn visit_start_section(&mut self, _cx: &VisitCtx<'a>, _start: &ComponentStartFunction) {}
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
    NA
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
            Space::NA => Self::NA,
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
    pub(crate) curr_item: (ItemKind, Option<usize>),
    pub(crate) inner: VisitCtxInner<'a>,
}
impl<'a> VisitCtx<'a> {
    pub(crate) fn new(component: &'a Component<'a>) -> Self {
        Self {
            curr_item: (ItemKind::Comp, None),
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
    Component(u32, &'a Component<'b>),
    Module(u32, &'a Module<'b>),

    Func(u32, &'a CanonicalFunction),
    CompType(u32, &'a ComponentType<'b>),
    CompInst(u32, &'a ComponentInstance<'b>),
    CoreInst(u32, &'a Instance<'b>),
    CoreType(u32, &'a CoreType<'b>),

    Alias(u32, &'a ComponentAlias<'b>),
    Import(u32, &'a ComponentImport<'b>),
    Export(u32, &'a ComponentExport<'b>)
}
