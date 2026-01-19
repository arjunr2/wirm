use crate::ir::component::section::ComponentSection;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Debug;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport,
    ComponentExternalKind, ComponentFuncType, ComponentImport, ComponentInstance,
    ComponentInstantiationArg, ComponentOuterAliasKind, ComponentStartFunction, ComponentType,
    ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CompositeInnerType,
    CompositeType, ContType, CoreType, Export, ExternalKind, FieldType, Import, Instance,
    InstanceTypeDeclaration, InstantiationArg, InstantiationArgKind, ModuleTypeDeclaration,
    OuterAliasKind, RecGroup, RefType, StorageType, SubType, TagType, TypeRef, ValType,
    VariantCase,
};

pub(crate) type SpaceId = usize;

/// Every IR node can have a reference to this to allow for instrumentation
/// to have access to the index stores and perform manipulations!
pub(crate) type StoreHandle = Rc<RefCell<IndexStore>>;

#[derive(Default, Debug)]
pub(crate) struct IndexStore {
    pub scopes: HashMap<SpaceId, IndexScope>,
    next_id: usize,
}
impl IndexStore {
    pub fn new_scope(&mut self) -> SpaceId {
        let id = self.use_next_id();
        self.scopes.insert(id, IndexScope::new(id));

        id
    }
    pub fn reset_indices(&mut self) {
        for scope in self.scopes.values_mut() {
            scope.reset_ids();
        }
    }
    pub fn index_from_assumed_id(&mut self, id: &SpaceId, r: &IndexedRef) -> (SpaceSubtype, usize) {
        self.get_mut(id).index_from_assumed_id(r)
    }
    pub fn reset_ids(&mut self, id: &SpaceId) {
        self.get_mut(id).reset_ids()
    }
    pub fn assign_actual_id(
        &mut self,
        id: &SpaceId,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
    ) {
        self.get_mut(id).assign_actual_id(space, section, vec_idx)
    }
    pub fn assign_assumed_id(
        &mut self,
        id: &SpaceId,
        space: &Space,
        section: &ComponentSection,
        curr_idx: usize,
    ) -> Option<usize> {
        self.get_mut(id).assign_assumed_id(space, section, curr_idx)
    }

    pub fn assign_assumed_id_for<I: Debug + IndexSpaceOf>(
        &mut self,
        id: &SpaceId,
        items: &Vec<I>,
        curr_idx: usize,
        sections: &Vec<ComponentSection>,
    ) {
        self.get_mut(id)
            .assign_assumed_id_for(items, curr_idx, sections)
    }
    fn use_next_id(&mut self) -> SpaceId {
        let next = self.next_id;
        self.next_id += 1;

        next
    }

    fn get_mut(&mut self, id: &SpaceId) -> &mut IndexScope {
        self.scopes.get_mut(id).unwrap()
    }
}

/// A single lexical index scope in a WebAssembly component.
///
/// An `IndexScope` contains all index spaces that are *visible at one level*
/// of the component hierarchy. Each scope corresponds to a lexical boundary
/// introduced by constructs such as:
///
/// - a `component`
/// - a `component type`
/// - a `component instance`
///
/// Within a scope, indices are allocated monotonically and are only valid
/// relative to that scope. Nested constructs introduce *new* `IndexScope`s,
/// which may reference items in outer scopes via `(outer N ...)` declarations.
///
/// ## Relationship to the Component Model
///
/// In the WebAssembly Component Model, index spaces are *lexically scoped*.
/// For example:
///
/// - Component functions, values, instances, and types each have their own
///   index spaces.
/// - Core index spaces (functions, types, memories, etc.) are also scoped when
///   introduced at the component level.
/// - Entering a nested component (or component type / instance) creates a new
///   set of index spaces that shadow outer ones.
///
/// `IndexScope` models exactly one such lexical level.
///
/// ## Scope Stack Usage
///
/// `IndexScope` is intended to be used in conjunction with a stack structure
/// (e.g. `ScopeStack`), where:
///
/// - entering a nested construct pushes a new `IndexScope`
/// - exiting the construct pops it
/// - resolving `(outer depth ...)` references indexes into the stack by depth
///
/// This design allows encode-time traversal to correctly reindex references
/// even when IR nodes are visited in an arbitrary order (e.g. during
/// instrumentation).
///
/// ## Encode-Time Semantics
///
/// During encoding, the active `IndexScope` determines:
///
/// - where newly declared items are allocated
/// - how referenced indices are remapped
/// - which outer scope to consult for `(outer ...)` references
///
/// `IndexScope` does **not** represent all index spaces in the component;
/// it represents only those visible at a single lexical level.
///
/// We build these index spaces following the order of the original IR, then traverse the IR out-of-order
/// based on the instrumentation injections, we must enable the lookup of spaces through assigned IDs. This
/// ensures that we do not use the wrong index space for a node in a reordered list of IR nodes.
///
///
/// ## Design Note
///
/// This type intentionally separates *scope structure* from *IR structure*.
/// IR nodes do not own scopes; instead, scopes are entered and exited explicitly
/// during traversal. This keeps index resolution explicit, debuggable, and
/// faithful to the specification.
#[derive(Clone, Debug, Default)]
pub(crate) struct IndexScope {
    pub(crate) id: SpaceId,

    // Component-level spaces
    pub comp_func: IdxSpace,
    pub comp_val: IdxSpace,
    pub comp_type: IdxSpace,
    pub comp_inst: IdxSpace,

    // Core space (added by component model)
    pub core_inst: IdxSpace, // (these are module instances)
    pub module: IdxSpace,

    // Core spaces that exist at the component-level
    pub core_type: IdxSpace,
    pub core_func: IdxSpace, // these are canonical function decls!
    pub core_memory: IdxSpace,
    pub core_table: IdxSpace,
    pub core_global: IdxSpace,
    pub core_tag: IdxSpace,

    // General trackers for indices of item vectors (used during encoding to see where i've been)
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
impl IndexScope {
    pub fn new(id: SpaceId) -> Self {
        Self {
            id,
            comp_func: IdxSpace::new("component_functions".to_string()),
            comp_val: IdxSpace::new("component_values".to_string()),
            comp_type: IdxSpace::new("component_types".to_string()),
            comp_inst: IdxSpace::new("component_instances".to_string()),
            core_inst: IdxSpace::new("core_instances".to_string()),
            module: IdxSpace::new("core_modules".to_string()),

            core_type: IdxSpace::new("core_types".to_string()),
            core_func: IdxSpace::new("core_functions".to_string()),
            core_table: IdxSpace::new("core_tables".to_string()),
            core_memory: IdxSpace::new("core_memories".to_string()),
            core_global: IdxSpace::new("core_globals".to_string()),
            core_tag: IdxSpace::new("core_tags".to_string()),
            ..Self::default()
        }
    }

    /// This function is called as I parse a component. This is necessary since different items encoded
    /// in a component index into different namespaces. There is not a one-to-one relationship between
    /// those items' indices in a vector to the index space it manipulates!
    ///
    /// Consider a canonical function, this can take place of an index in the core-function OR the
    /// component-function index space!
    pub fn assign_assumed_id_for<I: Debug + IndexSpaceOf>(
        &mut self,
        items: &Vec<I>,
        curr_idx: usize,
        sections: &Vec<ComponentSection>, // one per item
    ) {
        debug_assert_eq!(items.len(), sections.len());
        for ((i, item), section) in items.iter().enumerate().zip(sections) {
            self.assign_assumed_id(&item.index_space_of(), section, curr_idx + i);
        }
    }

    /// This is also called as I parse a component for the same reason mentioned above in the documentation for [`IdxSpaces.assign_assumed_id_for`].
    pub fn assign_assumed_id(
        &mut self,
        space: &Space,
        section: &ComponentSection,
        curr_idx: usize,
    ) -> Option<usize> {
        if let Some(space) = self.get_space_mut(space) {
            Some(space.assign_assumed_id(section, curr_idx))
        } else {
            None
        }
    }

    pub fn lookup_assumed_id(
        &self,
        space: &Space,
        section: &ComponentSection,
        vec_idx: usize,
    ) -> usize {
        if let Some(space) = self.get_space(space) {
            if let Some(assumed_id) = space.lookup_assumed_id(section, vec_idx) {
                return *assumed_id;
            }
        }
        panic!("[{:?}] No assumed ID for index: {}", space, vec_idx)
    }

    pub fn index_from_assumed_id(&mut self, r: &IndexedRef) -> (SpaceSubtype, usize) {
        if let Some(space) = self.get_space_mut(&r.space) {
            if let Some((ty, idx)) = space.index_from_assumed_id(r.index as usize) {
                return (ty, idx);
            } else {
                println!("couldn't find idx");
            }
        } else {
            println!("couldn't find space");
        }
        panic!(
            "[{:?}@scope{}] No index for assumed ID: {}",
            r.space, self.id, r.index
        )
    }

    pub fn assign_actual_id(&mut self, space: &Space, section: &ComponentSection, vec_idx: usize) {
        let assumed_id = self.lookup_assumed_id(space, section, vec_idx);
        if let Some(space) = self.get_space_mut(space) {
            space.assign_actual_id(assumed_id);
        }
    }

    pub fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
        if let Some(space) = self.get_space(&r.space) {
            if let Some(actual_id) = space.lookup_actual_id(r.index as usize) {
                return *actual_id;
            }
        }
        panic!(
            "[{:?}] Can't find assumed id {} in id-tracker",
            r.space, r.index
        );
    }

    /// This function is used while encoding the component. This means that we
    /// should already know the space ID associated with the component section
    /// (if in visiting this next session we enter some inner index space).
    ///
    /// So, we use the associated space ID to return the inner index space. The
    /// calling function should use this return value to then context switch into
    /// this new index space. When we've finished visiting the section, swap back
    /// to the returned index space's `parent` (a field on the space).
    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        let tracker = match section {
            ComponentSection::Component => {
                // CREATES A NEW IDX SPACE SCOPE
                &mut self.last_processed_component
            }
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

    pub fn reset_ids(&mut self) {
        self.comp_func.reset_ids();
        self.comp_val.reset_ids();
        self.comp_type.reset_ids();
        self.comp_inst.reset_ids();

        self.core_inst.reset_ids();
        self.module.reset_ids();

        self.core_type.reset_ids();
        self.core_func.reset_ids();
        self.core_table.reset_ids();
        self.core_memory.reset_ids();
        self.core_global.reset_ids();
        self.core_tag.reset_ids();
    }

    // ===================
    // ==== UTILITIES ====
    // ===================

    fn get_space_mut(&mut self, space: &Space) -> Option<&mut IdxSpace> {
        let s = match space {
            Space::CompFunc => &mut self.comp_func,
            Space::CompVal => &mut self.comp_val,
            Space::CompType => &mut self.comp_type,
            Space::CompInst => &mut self.comp_inst,
            Space::CoreInst => &mut self.core_inst,
            Space::CoreModule => &mut self.module,
            Space::CoreType => &mut self.core_type,
            Space::CoreFunc => &mut self.core_func,
            Space::CoreMemory => &mut self.core_memory,
            Space::CoreTable => &mut self.core_table,
            Space::CoreGlobal => &mut self.core_global,
            Space::CoreTag => &mut self.core_tag,
        };
        Some(s)
    }

    fn get_space(&self, space: &Space) -> Option<&IdxSpace> {
        let s = match space {
            Space::CompFunc => &self.comp_func,
            Space::CompVal => &self.comp_val,
            Space::CompType => &self.comp_type,
            Space::CompInst => &self.comp_inst,
            Space::CoreInst => &self.core_inst,
            Space::CoreModule => &self.module,
            Space::CoreType => &self.core_type,
            Space::CoreFunc => &self.core_func,
            Space::CoreMemory => &self.core_memory,
            Space::CoreTable => &self.core_table,
            Space::CoreGlobal => &self.core_global,
            Space::CoreTag => &self.core_tag,
        };
        Some(s)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IdxSpace {
    /// The name of this index space (primarily for debugging purposes)
    name: String,
    /// This is the current ID that we've reached associated with this index space.
    current_id: usize,

    /// This is used at encode time. It tracks the actual ID that has been assigned
    /// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
    /// This is important since we know what ID should be associated with something only at encode time,
    /// since instrumentation has finished at that point and encoding of component items
    /// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
    actual_ids: HashMap<usize, usize>,

    /// Tracks the index in the MAIN item vector to the ID we've assumed for it: `main_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    main_assumed_ids: HashMap<usize, usize>,

    // The below maps are to track assumed IDs for item vectors that index into this index space.
    /// Tracks the index in the ALIAS item vector to the ID we've assumed for it: `alias_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    alias_assumed_ids: HashMap<usize, usize>,
    /// Tracks the index in the IMPORT item vector to the ID we've assumed for it: `imports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    imports_assumed_ids: HashMap<usize, usize>,
    /// Tracks the index in the EXPORT item vector to the ID we've assumed for it: `exports_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    exports_assumed_ids: HashMap<usize, usize>,

    /// (Only relevant for component_types)
    /// Tracks the index in the COMPONENT item vector to the ID we've assumed for it: `component_idx -> assumed_id`
    /// This ID will be used to reference that item in the IR.
    components_assumed_ids: HashMap<usize, usize>,

    index_from_assumed_id_cache: HashMap<usize, (SpaceSubtype, usize)>,
}
impl IdxSpace {
    pub fn new(name: String) -> Self {
        Self {
            name,
            ..Default::default()
        }
    }

    pub fn reset_ids(&mut self) {
        self.current_id = 0;
    }

    pub fn curr_id(&self) -> usize {
        // This returns the ID that we've reached thus far while encoding
        self.current_id
    }

    pub fn assign_actual_id(&mut self, assumed_id: usize) {
        let id = self.curr_id();

        self.actual_ids.insert(assumed_id, id);
        self.next();
    }

    fn next(&mut self) -> usize {
        let curr = self.current_id;
        self.current_id += 1;
        curr
    }

    pub fn lookup_assumed_id(&self, section: &ComponentSection, vec_idx: usize) -> Option<&usize> {
        let (_group, vector) = match section {
            ComponentSection::ComponentImport => ("imports", &self.imports_assumed_ids),
            ComponentSection::ComponentExport => ("exports", &self.exports_assumed_ids),
            ComponentSection::Alias => ("aliases", &self.alias_assumed_ids),
            ComponentSection::Component => ("components", &self.components_assumed_ids),

            ComponentSection::Module
            | ComponentSection::CoreType
            | ComponentSection::ComponentType
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => ("main", &self.main_assumed_ids),
        };

        vector.get(&vec_idx)
    }

    pub fn index_from_assumed_id(&mut self, assumed_id: usize) -> Option<(SpaceSubtype, usize)> {
        if let Some(cached_data) = self.index_from_assumed_id_cache.get(&assumed_id) {
            return Some(*cached_data);
        }

        // We haven't cached this yet, we must do the less efficient logic and do a full lookup,
        // then we can cache what we find!
        let maps = vec![
            (SpaceSubtype::Main, &self.main_assumed_ids),
            (SpaceSubtype::Import, &self.imports_assumed_ids),
            (SpaceSubtype::Export, &self.exports_assumed_ids),
            (SpaceSubtype::Alias, &self.alias_assumed_ids),
            (SpaceSubtype::Components, &self.components_assumed_ids),
        ];

        for (subty, map) in maps.iter() {
            for (idx, assumed) in map.iter() {
                if *assumed == assumed_id {
                    let result = (*subty, *idx);
                    // cache what we found
                    self.index_from_assumed_id_cache
                        .insert(assumed_id, result.clone());

                    return Some(result);
                }
            }
        }
        None
    }

    pub fn assign_assumed_id(&mut self, section: &ComponentSection, vec_idx: usize) -> usize {
        let assumed_id = self.curr_id();
        self.next();
        let to_update = match section {
            ComponentSection::ComponentImport => &mut self.imports_assumed_ids,
            ComponentSection::ComponentExport => &mut self.exports_assumed_ids,
            ComponentSection::Alias => &mut self.alias_assumed_ids,
            ComponentSection::Component => &mut self.components_assumed_ids,

            ComponentSection::Module
            | ComponentSection::CoreType
            | ComponentSection::ComponentType
            | ComponentSection::CoreInstance
            | ComponentSection::ComponentInstance
            | ComponentSection::Canon
            | ComponentSection::CustomSection
            | ComponentSection::ComponentStartSection => &mut self.main_assumed_ids,
        };
        to_update.insert(vec_idx, assumed_id);

        assumed_id
    }

    pub fn lookup_actual_id(&self, id: usize) -> Option<&usize> {
        self.actual_ids.get(&id)
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum SpaceSubtype {
    Export,
    Import,
    Alias,
    // This is only relevant for component types!
    Components,
    Main,
}

// Logic to figure out which index space is being manipulated
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Space {
    // Component-level spaces
    CompFunc,
    CompVal,
    CompType,
    CompInst,

    // Core-level spaces
    CoreInst,
    CoreModule,
    CoreType,
    CoreFunc,
    CoreMemory,
    CoreTable,
    CoreGlobal,
    CoreTag,
}

pub trait NameOf {
    fn name_of(&self) -> String;
}
impl NameOf for ComponentExport<'_> {
    fn name_of(&self) -> String {
        self.name.0.to_string()
    }
}

// Trait for centralizing index space mapping
pub trait IndexSpaceOf {
    fn index_space_of(&self) -> Space;
}

impl IndexSpaceOf for ComponentTypeRef {
    fn index_space_of(&self) -> Space {
        // This is the index space to use when looking up
        // the IDs in this ref.
        match self {
            Self::Value(_) => Space::CompVal,
            Self::Instance(_) => Space::CompInst,
            Self::Component(_) => Space::CompType,
            Self::Module(_) => Space::CoreModule,
            Self::Func(_) | Self::Type(_) => Space::CompType,
        }
    }
}

impl IndexSpaceOf for ComponentImport<'_> {
    fn index_space_of(&self) -> Space {
        // This is the index space of THIS IMPORT!
        // Not what space to use for the IDs of the typeref!
        match self.ty {
            ComponentTypeRef::Func(_) => Space::CompFunc,
            ComponentTypeRef::Value(_) => Space::CompVal,
            ComponentTypeRef::Type(_) => Space::CompType,
            ComponentTypeRef::Instance(_) => Space::CompInst,
            ComponentTypeRef::Component(_) => Space::CompInst,
            ComponentTypeRef::Module(_) => Space::CoreModule,
        }
    }
}

impl IndexSpaceOf for ComponentExport<'_> {
    fn index_space_of(&self) -> Space {
        // This is the index space of THIS EXPORT!
        // Not what space to use for the IDs of the typeref!
        match self.kind {
            ComponentExternalKind::Module => Space::CoreModule,
            ComponentExternalKind::Func => Space::CompFunc,
            ComponentExternalKind::Value => Space::CompVal,
            ComponentExternalKind::Type => Space::CompType,
            ComponentExternalKind::Instance => Space::CompInst,
            ComponentExternalKind::Component => Space::CompInst,
        }
    }
}

impl IndexSpaceOf for Instance<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreInst
    }
}

impl<'a> IndexSpaceOf for ComponentAlias<'a> {
    fn index_space_of(&self) -> Space {
        match self {
            // Aliasing an export of a component instance
            ComponentAlias::InstanceExport { kind, .. } => match kind {
                ComponentExternalKind::Func => Space::CompFunc,
                ComponentExternalKind::Value => Space::CompVal,
                ComponentExternalKind::Type => Space::CompType,
                ComponentExternalKind::Instance => Space::CompInst,
                ComponentExternalKind::Component => Space::CompType,
                ComponentExternalKind::Module => Space::CoreModule,
            },

            // Aliasing an export of a core instance
            ComponentAlias::CoreInstanceExport { kind, .. } => match kind {
                ExternalKind::Func => Space::CoreFunc,
                ExternalKind::Memory => Space::CoreMemory,
                ExternalKind::Table => Space::CoreTable,
                ExternalKind::Global => Space::CoreGlobal,
                ExternalKind::Tag => Space::CoreTag,
                ExternalKind::FuncExact => Space::CoreFunc,
            },

            // Aliasing an outer item
            ComponentAlias::Outer { kind, .. } => match kind {
                ComponentOuterAliasKind::CoreModule => Space::CoreModule,
                ComponentOuterAliasKind::CoreType => Space::CoreType,
                ComponentOuterAliasKind::Type => Space::CompType,
                ComponentOuterAliasKind::Component => Space::CompType,
            },
        }
    }
}

impl IndexSpaceOf for CanonicalFunction {
    fn index_space_of(&self) -> Space {
        match self {
            CanonicalFunction::Lower { .. } => Space::CoreFunc,
            CanonicalFunction::Lift { .. } => Space::CompFunc,

            // TODO: These actually don't create core functions!
            // I'm just doing this as a workaround. The core function
            // is generated IF the IR node is referenced and exported
            // somehow...
            // Resource-related functions reference a resource type
            CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. } => Space::CoreFunc,

            // Thread spawn / new indirect → function type
            CanonicalFunction::ThreadSpawnRef { .. }
            | CanonicalFunction::ThreadSpawnIndirect { .. }
            | CanonicalFunction::ThreadNewIndirect { .. } => Space::CompFunc,

            // Task-related functions operate on values
            CanonicalFunction::TaskReturn { .. }
            | CanonicalFunction::TaskCancel { .. }
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::SubtaskCancel { .. } => Space::CompFunc,

            // Context access
            CanonicalFunction::ContextGet(_) | CanonicalFunction::ContextSet(_) => Space::CompFunc,

            // Stream / Future functions operate on types
            CanonicalFunction::StreamNew { .. }
            | CanonicalFunction::StreamRead { .. }
            | CanonicalFunction::StreamWrite { .. }
            | CanonicalFunction::StreamCancelRead { .. }
            | CanonicalFunction::StreamCancelWrite { .. }
            | CanonicalFunction::StreamDropReadable { .. }
            | CanonicalFunction::StreamDropWritable { .. }
            | CanonicalFunction::FutureNew { .. }
            | CanonicalFunction::FutureRead { .. }
            | CanonicalFunction::FutureWrite { .. }
            | CanonicalFunction::FutureCancelRead { .. }
            | CanonicalFunction::FutureCancelWrite { .. }
            | CanonicalFunction::FutureDropReadable { .. }
            | CanonicalFunction::FutureDropWritable { .. } => Space::CompFunc,

            // Error context → operate on values
            CanonicalFunction::ErrorContextNew { .. }
            | CanonicalFunction::ErrorContextDebugMessage { .. }
            | CanonicalFunction::ErrorContextDrop => Space::CompFunc,

            // Waitable set → memory
            CanonicalFunction::WaitableSetWait { .. }
            | CanonicalFunction::WaitableSetPoll { .. } => Space::CompFunc,
            CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin => Space::CompFunc,

            // Thread functions
            CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo { .. }
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::ThreadAvailableParallelism => Space::CompFunc,

            CanonicalFunction::BackpressureSet
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec => Space::CompFunc,
        }
    }
}

impl IndexSpaceOf for Module<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreModule
    }
}

impl IndexSpaceOf for Component<'_> {
    fn index_space_of(&self) -> Space {
        Space::CompType
    }
}

impl IndexSpaceOf for CoreType<'_> {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for ComponentType<'_> {
    fn index_space_of(&self) -> Space {
        Space::CompType
    }
}

impl IndexSpaceOf for ComponentInstance<'_> {
    fn index_space_of(&self) -> Space {
        Space::CompInst
    }
}

impl IndexSpaceOf for InstantiationArgKind {
    fn index_space_of(&self) -> Space {
        match self {
            InstantiationArgKind::Instance => Space::CoreInst,
        }
    }
}

impl IndexSpaceOf for ExternalKind {
    fn index_space_of(&self) -> Space {
        match self {
            ExternalKind::Func => Space::CoreFunc,
            ExternalKind::Table => Space::CoreTable,
            ExternalKind::Memory => Space::CoreMemory,
            ExternalKind::Global => Space::CoreGlobal,
            ExternalKind::Tag => Space::CoreTag,
            ExternalKind::FuncExact => Space::CoreFunc,
        }
    }
}

impl IndexSpaceOf for ComponentExternalKind {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentExternalKind::Func => Space::CompFunc,
            ComponentExternalKind::Value => Space::CompVal,
            ComponentExternalKind::Type => Space::CompType,
            ComponentExternalKind::Instance => Space::CompInst,
            ComponentExternalKind::Component => Space::CompType,
            ComponentExternalKind::Module => Space::CoreModule,
        }
    }
}

impl IndexSpaceOf for ComponentOuterAliasKind {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentOuterAliasKind::CoreModule => Space::CoreModule,
            ComponentOuterAliasKind::CoreType => Space::CoreType,
            ComponentOuterAliasKind::Type => Space::CompType,
            ComponentOuterAliasKind::Component => Space::CompType,
        }
    }
}

impl IndexSpaceOf for ComponentTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ty.index_space_of(),
            ComponentTypeDeclaration::Type(ty) => ty.index_space_of(),
            ComponentTypeDeclaration::Alias(alias) => alias.index_space_of(),
            ComponentTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
            ComponentTypeDeclaration::Import(import) => import.index_space_of(),
        }
    }
}

impl IndexSpaceOf for InstanceTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            InstanceTypeDeclaration::CoreType(ty) => ty.index_space_of(),
            InstanceTypeDeclaration::Type(ty) => ty.index_space_of(),
            InstanceTypeDeclaration::Alias(a) => a.index_space_of(),
            InstanceTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
        }
    }
}

impl IndexSpaceOf for ModuleTypeDeclaration<'_> {
    fn index_space_of(&self) -> Space {
        match self {
            ModuleTypeDeclaration::Type(_) => Space::CoreType,
            ModuleTypeDeclaration::Export { ty, .. } => ty.index_space_of(),
            ModuleTypeDeclaration::OuterAlias { kind, .. } => kind.index_space_of(),
            ModuleTypeDeclaration::Import(Import { ty, .. }) => ty.index_space_of(),
        }
    }
}

impl IndexSpaceOf for TypeRef {
    fn index_space_of(&self) -> Space {
        Space::CoreType
    }
}

impl IndexSpaceOf for OuterAliasKind {
    fn index_space_of(&self) -> Space {
        match self {
            OuterAliasKind::Type => Space::CoreType,
        }
    }
}

/// To unify how I look up the referenced indices inside an IR node
pub trait ReferencedIndices {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs>;
}

#[derive(Default)]
pub struct Refs {
    pub comp: Option<IndexedRef>,
    pub inst: Option<IndexedRef>,
    pub module: Option<IndexedRef>,
    pub func: Option<IndexedRef>,
    pub ty: Option<IndexedRef>,
    pub val: Option<IndexedRef>,
    pub mem: Option<IndexedRef>,
    pub table: Option<IndexedRef>,
    pub misc: Option<IndexedRef>,
    pub params: Vec<Option<Refs>>,
    pub results: Vec<Option<Refs>>,
    pub others: Vec<Option<Refs>>,
}
impl Refs {
    pub fn comp(&self) -> &IndexedRef {
        self.comp.as_ref().unwrap()
    }
    pub fn inst(&self) -> &IndexedRef {
        self.inst.as_ref().unwrap()
    }
    pub fn module(&self) -> &IndexedRef {
        self.module.as_ref().unwrap()
    }
    pub fn func(&self) -> &IndexedRef {
        self.func.as_ref().unwrap()
    }
    pub fn ty(&self) -> &IndexedRef {
        self.ty.as_ref().unwrap()
    }
    pub fn val(&self) -> &IndexedRef {
        self.val.as_ref().unwrap()
    }
    pub fn mem(&self) -> &IndexedRef {
        self.mem.as_ref().unwrap()
    }
    pub fn table(&self) -> &IndexedRef {
        self.table.as_ref().unwrap()
    }
    pub fn misc(&self) -> &IndexedRef {
        self.misc.as_ref().unwrap()
    }
    pub fn params(&self) -> &Vec<Option<Refs>> {
        &self.params
    }
    pub fn results(&self) -> &Vec<Option<Refs>> {
        &self.results
    }
    pub fn others(&self) -> &Vec<Option<Refs>> {
        &self.others
    }

    pub fn as_list(&self) -> Vec<IndexedRef> {
        let mut res = vec![];
        let Refs {
            comp,
            inst,
            module,
            func,
            ty,
            val,
            mem,
            table,
            misc,
            params,
            results,
            others,
        } = self;

        if let Some(comp) = comp {
            res.push(*comp);
        }
        if let Some(inst) = inst {
            res.push(*inst);
        }
        if let Some(module) = module {
            res.push(*module);
        }
        if let Some(func) = func {
            res.push(*func);
        }
        if let Some(ty) = ty {
            res.push(*ty);
        }
        if let Some(val) = val {
            res.push(*val);
        }
        if let Some(mem) = mem {
            res.push(*mem);
        }
        if let Some(table) = table {
            res.push(*table);
        }
        if let Some(misc) = misc {
            res.push(*misc);
        }
        params.iter().for_each(|o| {
            if let Some(o) = o {
                res.extend(o.as_list());
            }
        });
        results.iter().for_each(|o| {
            if let Some(o) = o {
                res.extend(o.as_list());
            }
        });
        others.iter().for_each(|o| {
            if let Some(o) = o {
                res.extend(o.as_list());
            }
        });

        res
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Depth(i32);
impl Depth {
    pub fn val(&self) -> i32 {
        self.0
    }
    pub fn is_inner(&self) -> bool {
        self.0 < 0
    }
    pub fn inner(mut self) -> Self {
        self.0 -= 1;
        self
    }
    pub fn outer(mut self) -> Self {
        self.0 += 1;
        self
    }
    pub fn outer_at(mut self, depth: u32) -> Self {
        self.0 += depth as i32;
        self
    }
}

/// A single referenced index with semantic metadata
#[derive(Copy, Clone, Debug)]
pub struct IndexedRef {
    /// The depth of the index space scope to look this up in
    /// If positive, it's one level ABOVE the current scope (outer)
    /// If negative, it's one level DEEPER the current scope (inner)
    /// If zero, it's the current scope
    pub depth: Depth,
    pub space: Space,
    pub index: u32,
}

impl ReferencedIndices for Module<'_> {
    fn referenced_indices(&self, _: Depth) -> Option<Refs> {
        None
    }
}

impl ReferencedIndices for ComponentType<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentType::Defined(ty) => ty.referenced_indices(depth),
            ComponentType::Func(ty) => ty.referenced_indices(depth),
            ComponentType::Component(tys) => {
                let mut others = vec![];
                for ty in tys.iter() {
                    others.push(ty.referenced_indices(depth.inner()));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            ComponentType::Instance(tys) => {
                let mut others = vec![];
                for ty in tys.iter() {
                    others.push(ty.referenced_indices(depth.inner()));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            ComponentType::Resource { rep, dtor } => Some(Refs {
                ty: if let Some(refs) = rep.referenced_indices(depth) {
                    refs.ty
                } else {
                    None
                },
                func: dtor.map(|id| IndexedRef {
                    depth,
                    space: Space::CoreFunc,
                    index: id,
                }),
                ..Default::default()
            }),
        }
    }
}

impl ReferencedIndices for ComponentFuncType<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let mut params = vec![];
        for (_, ty) in self.params.iter() {
            params.push(ty.referenced_indices(depth));
        }
        let mut results = vec![];
        if let Some(ty) = self.result {
            results.push(ty.referenced_indices(depth));
        }
        Some(Refs {
            params,
            results,
            ..Default::default()
        })
    }
}

impl ReferencedIndices for ComponentDefinedType<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentDefinedType::Record(records) => {
                let mut others = vec![];
                for (_, ty) in records.iter() {
                    others.push(ty.referenced_indices(depth));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            ComponentDefinedType::Variant(variants) => {
                // Explanation of variants.refines:
                // This case `refines` (is a subtype/specialization of) another case in the same variant.
                // So the u32 refers to: the index of another case within the current variant’s case list.
                // It is NOT an index into some global index space (hence not handling it here)
                let mut others = vec![];
                for VariantCase {
                    name: _,
                    ty,
                    refines: _,
                } in variants.iter()
                {
                    if let Some(t) = ty {
                        let ty_refs: Option<Refs> = t.referenced_indices(depth);
                        others.push(ty_refs);
                    }
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            ComponentDefinedType::List(ty)
            | ComponentDefinedType::FixedSizeList(ty, _)
            | ComponentDefinedType::Option(ty) => ty.referenced_indices(depth),
            ComponentDefinedType::Tuple(tys) => {
                let mut others = vec![];
                for ty in tys.iter() {
                    others.push(ty.referenced_indices(depth));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            ComponentDefinedType::Primitive(_)
            | ComponentDefinedType::Enum(_)
            | ComponentDefinedType::Flags(_) => None,
            ComponentDefinedType::Result { ok, err } => {
                let ok_r = ok.and_then(|ty| ty.referenced_indices(depth));
                let err_r = err.and_then(|ty| ty.referenced_indices(depth));
                Some(Refs {
                    others: vec![ok_r, err_r],
                    ..Default::default()
                })
            }
            ComponentDefinedType::Own(ty) | ComponentDefinedType::Borrow(ty) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *ty,
                }),
                ..Default::default()
            }),
            ComponentDefinedType::Future(ty) | ComponentDefinedType::Stream(ty) => {
                if let Some(ty) = ty {
                    ty.referenced_indices(depth)
                } else {
                    None
                }
            }
        }
    }
}

impl ReferencedIndices for ComponentTypeDeclaration<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ty.referenced_indices(depth),
            ComponentTypeDeclaration::Type(ty) => ty.referenced_indices(depth),
            ComponentTypeDeclaration::Alias(ty) => ty.referenced_indices(depth),
            ComponentTypeDeclaration::Export { ty, .. } => ty.referenced_indices(depth),
            ComponentTypeDeclaration::Import(import) => import.referenced_indices(depth),
        }
    }
}

impl ReferencedIndices for InstanceTypeDeclaration<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            InstanceTypeDeclaration::CoreType(ty) => ty.referenced_indices(depth),
            InstanceTypeDeclaration::Type(ty) => ty.referenced_indices(depth),
            InstanceTypeDeclaration::Alias(ty) => ty.referenced_indices(depth),
            InstanceTypeDeclaration::Export { ty, .. } => ty.referenced_indices(depth),
        }
    }
}

impl ReferencedIndices for CoreType<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            CoreType::Rec(group) => group.referenced_indices(depth),
            CoreType::Module(tys) => {
                let mut others = vec![];
                for ty in tys.iter() {
                    others.push(ty.referenced_indices(depth.inner()));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
        }
    }
}

impl ReferencedIndices for RecGroup {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let mut others = vec![];
        self.types().for_each(|subty| {
            others.push(subty.referenced_indices(depth));
        });
        Some(Refs {
            others,
            ..Default::default()
        })
    }
}

impl ReferencedIndices for SubType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let mut others = vec![];
        others.push(self.composite_type.referenced_indices(depth));

        Some(Refs {
            ty: if let Some(packed) = self.supertype_idx {
                Some(IndexedRef {
                    depth,
                    space: Space::CoreType,
                    index: packed.unpack().as_module_index().unwrap(),
                })
            } else {
                None
            },
            others,
            ..Default::default()
        })
    }
}

impl ReferencedIndices for CompositeType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let mut others = vec![];

        others.push(self.inner.referenced_indices(depth));
        let desc_id = if let Some(descriptor) = self.descriptor_idx {
            Some(IndexedRef {
                depth,
                space: Space::CompType,
                index: descriptor.unpack().as_module_index().unwrap(),
            })
        } else {
            None
        };
        let describes_id = if let Some(describes) = self.describes_idx {
            Some(IndexedRef {
                depth,
                space: Space::CompType,
                index: describes.unpack().as_module_index().unwrap(),
            })
        } else {
            None
        };

        Some(Refs {
            ty: desc_id,
            misc: describes_id,
            others,
            ..Default::default()
        })
    }
}

impl ReferencedIndices for CompositeInnerType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            CompositeInnerType::Func(f) => {
                let mut params = vec![];
                for ty in f.params().iter() {
                    params.push(ty.referenced_indices(depth));
                }
                let mut results = vec![];
                for ty in f.results().iter() {
                    results.push(ty.referenced_indices(depth));
                }
                Some(Refs {
                    params,
                    results,
                    ..Default::default()
                })
            }
            CompositeInnerType::Array(a) => a.0.referenced_indices(depth),
            CompositeInnerType::Struct(s) => {
                let mut others = vec![];
                for ty in s.fields.iter() {
                    others.push(ty.referenced_indices(depth));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            CompositeInnerType::Cont(ContType(ty)) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: ty.unpack().as_module_index().unwrap(),
                }),
                ..Default::default()
            }),
        }
    }
}

impl ReferencedIndices for FieldType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        self.element_type.referenced_indices(depth)
    }
}

impl ReferencedIndices for StorageType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            StorageType::I8 | StorageType::I16 => None,
            StorageType::Val(value) => value.referenced_indices(depth),
        }
    }
}

impl ReferencedIndices for ModuleTypeDeclaration<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ModuleTypeDeclaration::Type(group) => group.referenced_indices(depth),
            ModuleTypeDeclaration::Export { ty, .. } => ty.referenced_indices(depth),
            ModuleTypeDeclaration::Import(i) => i.ty.referenced_indices(depth),
            ModuleTypeDeclaration::OuterAlias { kind, count, index } => Some(Refs {
                misc: Some(IndexedRef {
                    depth: depth.outer_at(*count),
                    space: kind.index_space_of(),
                    index: *index,
                }),
                ..Default::default()
            }),
        }
    }
}

impl ReferencedIndices for VariantCase<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let ty = self
            .ty
            .and_then(|ty| ty.referenced_indices(depth))
            .map(|refs| refs.ty().clone());

        let misc = self.refines.and_then(|index| {
            Some(IndexedRef {
                depth,
                space: Space::CompType,
                index,
            })
        });

        Some(Refs {
            ty,
            misc,
            ..Default::default()
        })
    }
}

impl ReferencedIndices for ValType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64 | ValType::V128 => None,
            ValType::Ref(r) => r.referenced_indices(depth),
        }
    }
}

impl ReferencedIndices for RefType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        Some(Refs {
            ty: Some(IndexedRef {
                depth,
                space: Space::CoreType,
                index: self
                    .type_index()
                    .unwrap()
                    .unpack()
                    .as_module_index()
                    .unwrap(),
            }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for CanonicalFunction {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            CanonicalFunction::Lift {
                core_func_index,
                type_index,
                options,
            } => {
                let mut others = vec![];
                for opt in options.iter() {
                    others.push(opt.referenced_indices(depth));
                }
                Some(Refs {
                    func: Some(IndexedRef {
                        depth,
                        space: Space::CoreFunc,
                        index: *core_func_index,
                    }),
                    ty: Some(IndexedRef {
                        depth,
                        space: Space::CompType,
                        index: *type_index,
                    }),
                    others,
                    ..Default::default()
                })
            }

            CanonicalFunction::Lower {
                func_index,
                options,
            } => {
                let mut others = vec![];
                for opt in options.iter() {
                    others.push(opt.referenced_indices(depth));
                }
                Some(Refs {
                    func: Some(IndexedRef {
                        depth,
                        space: Space::CompFunc,
                        index: *func_index,
                    }),
                    others,
                    ..Default::default()
                })
            }
            CanonicalFunction::ResourceNew { resource }
            | CanonicalFunction::ResourceDrop { resource }
            | CanonicalFunction::ResourceDropAsync { resource }
            | CanonicalFunction::ResourceRep { resource } => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *resource,
                }),
                ..Default::default()
            }),
            CanonicalFunction::ThreadSpawnIndirect {
                func_ty_index,
                table_index,
            }
            | CanonicalFunction::ThreadNewIndirect {
                func_ty_index,
                table_index,
            } => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *func_ty_index,
                }),
                table: Some(IndexedRef {
                    depth,
                    space: Space::CoreTable,
                    index: *table_index,
                }),
                ..Default::default()
            }),

            CanonicalFunction::ThreadSpawnRef { func_ty_index } => Some(Refs {
                func: Some(IndexedRef {
                    depth,
                    space: Space::CompFunc,
                    index: *func_ty_index,
                }),
                ..Default::default()
            }),
            CanonicalFunction::TaskReturn { result, options } => {
                let mut others = vec![];
                for opt in options.iter() {
                    others.push(opt.referenced_indices(depth));
                }
                Some(Refs {
                    ty: if let Some(result) = result {
                        result.referenced_indices(depth).unwrap().ty
                    } else {
                        None
                    },
                    others,
                    ..Default::default()
                })
            }
            CanonicalFunction::StreamNew { ty }
            | CanonicalFunction::StreamDropReadable { ty }
            | CanonicalFunction::StreamDropWritable { ty }
            | CanonicalFunction::StreamCancelRead { ty, .. }
            | CanonicalFunction::StreamCancelWrite { ty, .. }
            | CanonicalFunction::FutureNew { ty }
            | CanonicalFunction::FutureDropReadable { ty }
            | CanonicalFunction::FutureDropWritable { ty }
            | CanonicalFunction::FutureCancelRead { ty, .. }
            | CanonicalFunction::FutureCancelWrite { ty, .. } => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *ty,
                }),
                ..Default::default()
            }),
            CanonicalFunction::StreamRead { ty, options }
            | CanonicalFunction::StreamWrite { ty, options }
            | CanonicalFunction::FutureRead { ty, options }
            | CanonicalFunction::FutureWrite { ty, options } => {
                let mut others = vec![];
                for opt in options.iter() {
                    others.push(opt.referenced_indices(depth));
                }
                Some(Refs {
                    ty: Some(IndexedRef {
                        depth,
                        space: Space::CompType,
                        index: *ty,
                    }),
                    others,
                    ..Default::default()
                })
            }
            CanonicalFunction::ErrorContextNew { options }
            | CanonicalFunction::ErrorContextDebugMessage { options } => {
                let mut others = vec![];
                for opt in options.iter() {
                    others.push(opt.referenced_indices(depth));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
            CanonicalFunction::WaitableSetWait { memory, .. }
            | CanonicalFunction::WaitableSetPoll { memory, .. } => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CoreMemory,
                    index: *memory,
                }),
                ..Default::default()
            }),
            CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadYieldTo { .. } => None,
            CanonicalFunction::ContextGet(i) | CanonicalFunction::ContextSet(i) => None,
            CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureSet
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::ErrorContextDrop
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadResumeLater => None,
        }
    }
}

impl ReferencedIndices for CanonicalOption {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            CanonicalOption::Memory(id) => Some(Refs {
                mem: Some(IndexedRef {
                    depth,
                    space: Space::CoreMemory,
                    index: *id,
                }),
                ..Default::default()
            }),
            CanonicalOption::Realloc(id)
            | CanonicalOption::PostReturn(id)
            | CanonicalOption::Callback(id) => Some(Refs {
                func: Some(IndexedRef {
                    depth,
                    space: Space::CoreFunc,
                    index: *id,
                }),
                ..Default::default()
            }),
            CanonicalOption::CoreType(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CoreType,
                    index: *id,
                }),
                ..Default::default()
            }),
            CanonicalOption::Async
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Gc
            | CanonicalOption::UTF8
            | CanonicalOption::UTF16 => None,
        }
    }
}

impl ReferencedIndices for ComponentImport<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        self.ty.referenced_indices(depth)
    }
}
impl ReferencedIndices for ComponentTypeRef {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match &self {
            // The reference is to a core module type.
            // The index is expected to be core type index to a core module type.
            ComponentTypeRef::Module(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CoreType,
                    index: *id,
                }),
                ..Default::default()
            }),
            ComponentTypeRef::Func(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *id,
                }),
                ..Default::default()
            }),
            ComponentTypeRef::Instance(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *id,
                }),
                ..Default::default()
            }),
            ComponentTypeRef::Component(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *id,
                }),
                ..Default::default()
            }),
            ComponentTypeRef::Value(ty) => ty.referenced_indices(depth),
            ComponentTypeRef::Type(_) => None,
        }
    }
}

impl ReferencedIndices for ComponentValType {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentValType::Primitive(_) => None,
            ComponentValType::Type(id) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompType,
                    index: *id,
                }),
                ..Default::default()
            }),
        }
    }
}

impl ReferencedIndices for ComponentInstantiationArg<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        Some(Refs {
            ty: Some(IndexedRef {
                depth,
                space: self.kind.index_space_of(),
                index: self.index,
            }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for ComponentExport<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef {
                depth,
                space: self.kind.index_space_of(),
                index: self.index,
            }),
            ty: if let Some(t) = &self.ty {
                t.referenced_indices(depth)?.ty
            } else {
                None
            },
            ..Default::default()
        })
    }
}

impl ReferencedIndices for Export<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef {
                depth,
                space: self.kind.index_space_of(),
                index: self.index,
            }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for InstantiationArg<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef {
                depth,
                space: self.kind.index_space_of(),
                index: self.index,
            }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for Instance<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            Instance::Instantiate { module_index, args } => {
                let mut others = vec![];
                // Recursively include indices from options
                for arg in args.iter() {
                    others.push(arg.referenced_indices(depth));
                }
                Some(Refs {
                    module: Some(IndexedRef {
                        depth,
                        space: Space::CoreModule,
                        index: *module_index,
                    }),
                    others,
                    ..Default::default()
                })
            }
            Instance::FromExports(exports) => {
                let mut others = vec![];
                // Recursively include indices from options
                for exp in exports.iter() {
                    others.push(exp.referenced_indices(depth));
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            }
        }
    }
}

impl ReferencedIndices for TypeRef {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            TypeRef::Func(ty)
            | TypeRef::Tag(TagType {
                kind: _,
                func_type_idx: ty,
            }) => Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CoreType,
                    index: *ty,
                }),
                ..Default::default()
            }),
            _ => None,
        }
    }
}

impl ReferencedIndices for ComponentAlias<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentAlias::InstanceExport { instance_index, .. } => Some(Refs {
                inst: Some(IndexedRef {
                    depth,
                    space: Space::CompInst,
                    index: *instance_index,
                }),
                ..Default::default()
            }),
            ComponentAlias::CoreInstanceExport { instance_index, .. } => Some(Refs {
                inst: Some(IndexedRef {
                    depth,
                    space: Space::CoreInst,
                    index: *instance_index,
                }),
                ..Default::default()
            }),
            ComponentAlias::Outer { count, index, kind } => Some(Refs {
                misc: Some(IndexedRef {
                    depth: depth.outer_at(*count),
                    space: kind.index_space_of(),
                    index: *index,
                }),
                ..Default::default()
            }),
        }
    }
}

impl ReferencedIndices for ComponentInstance<'_> {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        match self {
            ComponentInstance::Instantiate {
                component_index,
                args,
            } => {
                let mut others = vec![];
                // Recursively include indices from args
                for arg in args.iter() {
                    others.push(arg.referenced_indices(depth));
                }

                Some(Refs {
                    comp: Some(IndexedRef {
                        depth,
                        space: Space::CompType,
                        index: *component_index,
                    }),
                    others,
                    ..Default::default()
                })
            }

            ComponentInstance::FromExports(export) => {
                let mut others = vec![];
                // Recursively include indices from args
                for exp in export.iter() {
                    others.push(exp.referenced_indices(depth));
                }

                if !others.is_empty() {
                    Some(Refs {
                        others,
                        ..Default::default()
                    })
                } else {
                    None
                }
            }
        }
    }
}

impl ReferencedIndices for CustomSection<'_> {
    fn referenced_indices(&self, _: Depth) -> Option<Refs> {
        None
    }
}

impl ReferencedIndices for ComponentStartFunction {
    fn referenced_indices(&self, depth: Depth) -> Option<Refs> {
        let mut others = vec![];

        for v in self.arguments.iter() {
            others.push(Some(Refs {
                ty: Some(IndexedRef {
                    depth,
                    space: Space::CompVal,
                    index: *v,
                }),
                ..Default::default()
            }));
        }

        Some(Refs {
            func: Some(IndexedRef {
                depth,
                space: Space::CompFunc,
                index: self.func_index,
            }),
            others,
            ..Default::default()
        })
    }
}
