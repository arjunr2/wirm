//! ## Scope Tracking and Stable Component Identity
//!
//! This module defines the infrastructure used to safely track **nested index
//! spaces** across parsing, instrumentation, and encoding phases of a
//! WebAssembly *component*.
//!
//! WebAssembly components introduce hierarchical index scopes: components may
//! contain subcomponents, instances, types, and other constructs that form
//! their own index spaces. Additionally, `(outer ...)` references allow inner
//! scopes to refer to indices defined in enclosing scopes. Correctly resolving
//! these relationships at encode time therefore requires an explicit model of
//! scope nesting rather than a single flat index map.
//!
//! At the same time, this crate supports **component instrumentation**, meaning
//! the IR may be visited, transformed, and encoded in an order that does *not*
//! correspond to the original parse order. As a result, index resolution cannot
//! rely on traversal order alone.
//!
//! To address these constraints, this module separates **identity** from
//! **ownership** using two cooperating abstractions:
//!
//! ### `ScopeRegistry`
//!
//! `ScopeRegistry` is a central, shared registry that maps *IR node identity*
//! to the index scope (`SpaceId`) that the node owns or inhabits. This mapping is
//! established during parsing and maintained throughout the lifetime of the IR.
//!
//! Key properties:
//! - Index scopes are assigned **once**, during parsing or instrumentation.
//! - Lookups are performed later, during encoding, using stable identities.
//! - Nested scopes are resolved dynamically using an explicit **scope stack**
//!   rather than implicit traversal order.
//!
//! The registry intentionally operates on **raw pointers** (`*const T`) as
//! identity keys. This is safe under the invariants described below and avoids
//! coupling scope identity to Rust ownership or borrowing semantics.
//!
//! ### `ComponentHandle`
//!
//! `ComponentHandle` provides a **stable identity anchor** for a component after
//! it has been returned to the user. Once parsing completes, the `Component`
//! itself may be moved, wrapped, or otherwise owned by client code, which would
//! invalidate pointer-based identity tracking.
//!
//! `ComponentHandle` solves this by:
//! - Owning the `Component` behind an `Rc`
//! - Providing a stable allocation address for registry lookups
//! - Allowing internal encode logic to reliably recover the component’s
//!   associated `SpaceId`
//!
//! All other IR nodes remain owned *within* the `Component` and therefore do
//! not require handles; their addresses remain stable for the lifetime of the
//! component.
//!
//! ### Safety and Invariants
//!
//! This design relies on the following invariants:
//!
//! - All IR nodes (except the top-level component) are owned by the `Component`
//!   and are never moved after parsing.
//! - The top-level component’s identity is always accessed via
//!   `ComponentHandle`, never via `&Component`.
//! - `ScopeRegistry` entries are created during parsing and may be extended
//!   during instrumentation, but are never removed.
//! - Raw pointer usage is confined to **identity comparison only**; no pointer
//!   is ever dereferenced.
//!
//! These constraints allow the system to use otherwise “dangerous” primitives
//! (raw pointers, shared mutation) in a controlled and domain-specific way,
//! trading generality for correctness and debuggability.
//!
//! ### Design Tradeoffs
//!
//! This approach deliberately favors:
//! - Explicit scope modeling over implicit traversal
//! - Stable identity over borrow-driven lifetimes
//! - Simplicity and traceability over highly generic abstractions
//!
//! While this introduces some indirection and bookkeeping, it keeps the encode
//! logic understandable, debuggable, and resilient to future extensions of the
//! component model.
//!
//! In short: **index correctness is enforced structurally, not procedurally**.
//!

use crate::ir::component::idx_spaces::SpaceId;
use crate::ir::component::ComponentHandle;
use crate::ir::id::ComponentId;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::NonNull;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport,
    ComponentFuncType, ComponentImport, ComponentInstance, ComponentInstantiationArg,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef,
    ComponentValType, CompositeInnerType, CompositeType, CoreType, Export, FieldType, FuncType,
    Import, Instance, InstanceTypeDeclaration, InstantiationArg, ModuleTypeDeclaration,
    PrimitiveValType, RecGroup, RefType, StorageType, StructType, SubType, TypeRef, ValType,
    VariantCase,
};

/// A shared registry that maps IR node identity to the index scope it owns.
///
/// `ScopeRegistry` records the `SpaceId` associated with IR nodes that introduce
/// or participate in nested index spaces (e.g. components, instances, and
/// component types). Entries are created during parsing and may be extended
/// during instrumentation, then consulted during encoding to correctly resolve
/// scoped indices such as `(outer ...)` references.
///
/// The registry uses **raw pointer identity** (`*const T`) as lookup keys. This
/// is safe under the invariant that all registered nodes have stable addresses
/// for the lifetime of the component, and that pointers are never dereferenced—
/// only compared for identity.
///
/// This design decouples scope resolution from traversal order, allowing IR
/// instrumentation to visit and encode nodes in arbitrary order while still
/// producing correct index mappings.
///
/// # Debugging tips
///
/// If a scope lookup fails (e.g. `scope_entry(...)` returns `None`):
///
/// * **Check pointer identity**: ensure the same node instance is used for both
///   registration and lookup. Lookups must use the exact allocation that was
///   registered (for example, `Rc<Component>` vs `Component` will not match).
///
/// * **Verify registration timing**: the node must be registered *after* it is
///   fully constructed and before any encode-time lookups occur.
///
/// * **Confirm ownership invariants**: only nodes owned by the `Component`
///   should be registered. Moving a node out of the component or cloning it
///   will invalidate pointer-based lookups.
///
/// * **Log addresses**: printing `{:p}` for the registered pointer and the
///   lookup pointer is often the fastest way to identify mismatches.
///
/// These failures usually indicate a violation of the registry’s ownership or
/// lifetime assumptions rather than a logic error in index assignment itself.
#[derive(Default, Debug)]
pub(crate) struct IndexScopeRegistry {
    pub(crate) node_scopes: HashMap<NonNull<()>, ScopeEntry>,
}
impl IndexScopeRegistry {
    pub fn register<T: GetScopeKind>(&mut self, node: &T, space: SpaceId) {
        let ptr = NonNull::from(node).cast::<()>();
        let kind = node.scope_kind();
        assert_ne!(kind, ScopeOwnerKind::Unregistered);

        self.node_scopes.insert(ptr, ScopeEntry { space, kind });
    }

    pub fn scope_entry<T: GetScopeKind>(&self, node: &T) -> Option<ScopeEntry> {
        let ptr = NonNull::from(node).cast::<()>();

        if let Some(entry) = self.node_scopes.get(&ptr) {
            if entry.kind == node.scope_kind() {
                return Some(entry.clone());
            }
        }
        None
    }
}

/// Every IR node can have a reference to this to allow for instrumentation
/// to have access to the index scope mappings and perform manipulations!
pub(crate) type RegistryHandle = Rc<RefCell<IndexScopeRegistry>>;

#[derive(Debug, Clone, Copy)]
pub struct ScopeEntry {
    pub space: SpaceId,
    pub kind: ScopeOwnerKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeOwnerKind {
    /// A `(component ...)`
    Component,

    /// A core `(core type (module ...))`
    CoreTypeModule,

    /// A `(component type (component ...))`
    ComponentTypeComponent,
    /// A `(component type (instance ...))`
    ComponentTypeInstance,

    // Extend as needed
    Unregistered,
}

// impl ScopeOwnerKind {
//     pub(crate) fn from_section(value: &ComponentSection) -> Option<Self> {
//         match value {
//             ComponentSection::Component => Some(Self::Component),
//             ComponentSection::ComponentInstance
//             | ComponentSection::CoreType
//             | ComponentSection::ComponentImport
//             | ComponentSection::ComponentExport
//             | ComponentSection::CoreInstance
//             | ComponentSection::Canon
//             | ComponentSection::Alias
//             | ComponentSection::CustomSection
//             | ComponentSection::ComponentStartSection => None
//         }
//     }
// }

pub trait GetScopeKind {
    fn scope_kind(&self) -> ScopeOwnerKind {
        ScopeOwnerKind::Unregistered
    }
}
impl GetScopeKind for Component<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        ScopeOwnerKind::Component
    }
}
impl GetScopeKind for ComponentHandle<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        ScopeOwnerKind::Component
    }
}
impl GetScopeKind for CoreType<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        match self {
            CoreType::Module(_) => ScopeOwnerKind::CoreTypeModule,
            // other variants that do NOT introduce scopes should never be registered
            _ => ScopeOwnerKind::Unregistered,
        }
    }
}
impl GetScopeKind for ComponentType<'_> {
    fn scope_kind(&self) -> ScopeOwnerKind {
        match self {
            ComponentType::Component(_) => ScopeOwnerKind::ComponentTypeComponent,
            ComponentType::Instance(_) => ScopeOwnerKind::ComponentTypeInstance,
            ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {
                ScopeOwnerKind::Unregistered
            }
        }
    }
}
impl GetScopeKind for Module<'_> {}
impl GetScopeKind for ComponentTypeRef {}
impl GetScopeKind for ComponentDefinedType<'_> {}
impl GetScopeKind for ComponentFuncType<'_> {}
impl GetScopeKind for ComponentTypeDeclaration<'_> {}
impl GetScopeKind for InstanceTypeDeclaration<'_> {}
impl GetScopeKind for ComponentInstance<'_> {}
impl GetScopeKind for CanonicalFunction {}
impl GetScopeKind for ComponentAlias<'_> {}
impl GetScopeKind for ComponentImport<'_> {}
impl GetScopeKind for ComponentExport<'_> {}
impl GetScopeKind for Instance<'_> {}
impl GetScopeKind for ComponentStartFunction {}
impl GetScopeKind for CustomSection<'_> {}
impl GetScopeKind for ValType {}
impl GetScopeKind for ComponentInstantiationArg<'_> {}
impl GetScopeKind for CanonicalOption {}
impl GetScopeKind for ComponentValType {}
impl GetScopeKind for InstantiationArg<'_> {}
impl GetScopeKind for Export<'_> {}
impl GetScopeKind for PrimitiveValType {}
impl GetScopeKind for VariantCase<'_> {}
impl GetScopeKind for CompositeInnerType {}
impl GetScopeKind for FuncType {}
impl GetScopeKind for FieldType {}
impl GetScopeKind for StructType {}
impl GetScopeKind for CompositeType {}
impl GetScopeKind for StorageType {}
impl GetScopeKind for RefType {}
impl GetScopeKind for RecGroup {}
impl GetScopeKind for ModuleTypeDeclaration<'_> {}
impl GetScopeKind for Import<'_> {}
impl GetScopeKind for TypeRef {}
impl GetScopeKind for SubType {}

/// Assert that a node is registered in the `ScopeRegistry` at this point.
/// Panics if the node is not found.
/// This helps with debugging issues where a node may have been moved and
/// no longer upholds the invariants required by the scope lookup mechanism.
/// These checks will not be present in a release build, only debug builds, since
/// the check is encapsulated inside a `debug_assert_eq`.
#[macro_export]
macro_rules! assert_registered {
    ($registry:expr, $node:expr) => {{
        debug_assert!(
            $registry.borrow().scope_entry($node).is_some(),
            // concat!(
            "Debug assertion failed: node is not registered in ScopeRegistry: {:?}",
            $node // )
        );
    }};
}
#[macro_export]
macro_rules! assert_registered_with_id {
    ($registry:expr, $node:expr, $scope_id:expr) => {{
        debug_assert_eq!(
            $scope_id,
            $registry
                .borrow()
                .scope_entry($node)
                .expect(concat!(
                    "Debug assertion failed: node is not registered in ScopeRegistry: ",
                    stringify!($node)
                ))
                .space
        );
    }};
}

#[derive(Clone, Debug)]
pub struct ComponentStore<'a> {
    components: HashMap<ComponentId, &'a Component<'a>>,
}
impl<'a> ComponentStore<'a> {
    pub fn get(&self, id: &ComponentId) -> &'a Component<'a> {
        self.components.get(id).unwrap()
        // unsafe { ptr.cast::<Component<'a>>().as_ref() }
    }
}

pub fn build_component_store<'a>(root: &'a Component<'a>) -> ComponentStore<'a> {
    let mut map = HashMap::new();

    fn walk<'a>(comp: &'a Component<'a>, map: &mut HashMap<ComponentId, &'a Component<'a>>) {
        map.insert(comp.id, comp);
        for child in comp.components.iter() {
            walk(child, map);
        }
    }

    walk(root, &mut map);

    ComponentStore { components: map }
}
