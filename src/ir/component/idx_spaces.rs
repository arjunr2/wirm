use std::collections::HashMap;
use std::fmt::Debug;
use wasmparser::{CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentExternalKind, ComponentImport, ComponentInstance, ComponentInstantiationArg, ComponentOuterAliasKind, ComponentType, ComponentTypeRef, ComponentValType, CoreType, Export, ExternalKind, Instance, InstantiationArg, InstantiationArgKind, TagType, TypeRef, VariantCase};
use crate::ir::section::ComponentSection;
use crate::{Component, Module};

#[derive(Clone, Debug, Default)]
pub(crate) struct IdxSpaces {
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
    last_processed_custom: usize,
}
impl IdxSpaces {
    pub fn new() -> Self {
        Self {
            comp_func: IdxSpace::new("component_functions".to_string()),
            comp_val: IdxSpace::new("component_values".to_string()),
            comp_type: IdxSpace::new("component_types".to_string()),
            comp_inst: IdxSpace::new("component_instances".to_string()),
            // comp: IdxSpace::new("components".to_string()),

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
    pub fn assign_assumed_id_for<I: Debug + IndexSpaceOf>(&mut self, items: &Vec<I>, next_id: usize, section: &ComponentSection) {
        for (i, item) in items.iter().enumerate() {
            let curr_idx = next_id + i;
            self.assign_assumed_id(&item.index_space_of(), section, curr_idx);
        }
    }

    /// This is also called as I parse a component for the same reason mentioned above in the documentation for [`IdxSpaces.assign_assumed_id_for`].
    pub fn assign_assumed_id(&mut self, space: &Space, section: &ComponentSection, curr_idx: usize) -> Option<usize> {
        if let Some(space) = self.get_space_mut(space) {
            Some(space.assign_assumed_id(section, curr_idx))
        } else {
            None
        }
    }

    pub fn lookup_assumed_id(&self, space: &Space, section: &ComponentSection, vec_idx: usize) -> usize {
        if let Some(space) = self.new_get_space(space) {
            if let Some(assumed_id) = space.lookup_assumed_id(section, vec_idx) {
                return *assumed_id
            }
        }
        panic!("[{:?}] No assumed ID for index: {}", space, vec_idx)
    }

    pub fn index_from_assumed_id(&self, r: &IndexedRef) -> (SpaceSubtype, usize) {
        // TODO -- this is incredibly inefficient...i just want to move on with my life...
        if let Some(space) = self.new_get_space(&r.space) {
            if let Some((ty, idx)) = space.index_from_assumed_id(r.index as usize) {
                return (ty, idx)
            } else {
                println!("couldn't find idx");
            }
        } else {
            println!("couldn't find space");
        }
        panic!("[{:?}] No index for assumed ID: {}", r.space, r.index)
    }

    pub fn assign_actual_id(&mut self, space: &Space, section: &ComponentSection, vec_idx: usize) {
        let assumed_id = self.lookup_assumed_id(space, section, vec_idx);
        if let Some(space) = self.get_space_mut(space) {
            space.assign_actual_id(assumed_id);
        }
    }

    pub fn new_lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
        if let Some(space) = self.new_get_space(&r.space) {
            if let Some(actual_id) = space.lookup_actual_id(r.index as usize) {
                return *actual_id;
            }
        }
        panic!("[{:?}] Can't find assumed id {} in id-tracker", r.space, r.index);
    }

    pub fn lookup_actual_id_or_panic(&self, outer: &ComponentSection, inner: &ExternalItemKind, assumed_id: usize) -> usize {
        if let Some(space) = self.get_space(outer, inner) {
            if let Some(actual_id) = space.lookup_actual_id(assumed_id) {
                return *actual_id;
            }
        }
        panic!("[{:?}::{:?}] Can't find assumed id {assumed_id} in id-tracker", outer, inner);
    }

    pub fn visit_section(&mut self, section: &ComponentSection, num: usize) -> usize {
        let tracker = match section {
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
            ComponentSection::Component => &mut self.last_processed_component,
            ComponentSection::ComponentStartSection => panic!("No need to call this function for the start section!")
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

    fn new_get_space(&self, space: &Space) -> Option<&IdxSpace> {
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

    fn get_space(&self, outer: &ComponentSection, inner: &ExternalItemKind) -> Option<&IdxSpace> {
        let space = match outer {
            ComponentSection::Module => &self.module,
            ComponentSection::CoreType => &self.core_type,
            ComponentSection::ComponentType => &self.comp_type,
            ComponentSection::CoreInstance => &self.core_inst,
            ComponentSection::ComponentInstance => &self.comp_inst,
            ComponentSection::Canon => match inner {
                ExternalItemKind::CompFunc => &self.comp_func,
                ExternalItemKind::CoreFunc => &self.core_func,
                ExternalItemKind::CoreMemory => &self.core_memory,
                _ => panic!("shouldn't get here")
            },
            ComponentSection::Component => &self.comp_type,

            // These manipulate other index spaces!
            ComponentSection::Alias |
            ComponentSection::ComponentImport |
            ComponentSection::ComponentExport => match inner {
                ExternalItemKind::CompFunc => &self.comp_func,
                ExternalItemKind::CompVal => &self.comp_val,
                ExternalItemKind::CompType => &self.comp_type,
                ExternalItemKind::CompInst => &self.comp_inst,
                ExternalItemKind::Comp => &self.comp_type,
                ExternalItemKind::CoreInst => &self.core_inst,
                ExternalItemKind::Module => &self.module,
                ExternalItemKind::CoreType => &self.core_type,
                ExternalItemKind::CoreFunc => &self.core_func,
                ExternalItemKind::CoreTable => &self.core_table,
                ExternalItemKind::CoreMemory => &self.core_memory,
                ExternalItemKind::CoreGlobal => &self.core_global,
                ExternalItemKind::CoreTag => &self.core_tag,
                ExternalItemKind::NA => return None // nothing to do
            }
            ComponentSection::ComponentStartSection |
            ComponentSection::CustomSection => return None // nothing to do for custom or start sections
        };
        Some(space)
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
        let (group, vector) = match section {
            ComponentSection::ComponentImport => ("imports", &self.imports_assumed_ids),
            ComponentSection::ComponentExport => todo!(), // ("exports", &self.exports_assumed_ids),
            ComponentSection::Alias => ("aliases", &self.alias_assumed_ids),
            ComponentSection::Component => ("components", &self.components_assumed_ids),

            ComponentSection::Module |
            ComponentSection::CoreType |
            ComponentSection::ComponentType |
            ComponentSection::CoreInstance |
            ComponentSection::ComponentInstance |
            ComponentSection::Canon |
            ComponentSection::CustomSection |
            ComponentSection::ComponentStartSection => ("main", &self.main_assumed_ids)
        };

        vector.get(&vec_idx)
    }

    pub fn index_from_assumed_id(&self, assumed_id: usize) -> Option<(SpaceSubtype, usize)> {
        // TODO -- this is EXTREMELY inefficient!!
        // let (subty, map) = match section {
        //     ComponentSection::ComponentImport => (SpaceSubtype::Import, &self.imports_assumed_ids),
        //     ComponentSection::ComponentExport => (SpaceSubtype::Export, &self.exports_assumed_ids),
        //     ComponentSection::Alias => (SpaceSubtype::Alias, &self.alias_assumed_ids),
        //
        //     ComponentSection::Module |
        //     ComponentSection::CoreType |
        //     ComponentSection::ComponentType |
        //     ComponentSection::CoreInstance |
        //     ComponentSection::ComponentInstance |
        //     ComponentSection::Canon |
        //     ComponentSection::CustomSection |
        //     ComponentSection::Component |
        //     ComponentSection::ComponentStartSection => (SpaceSubtype::Main, &self.main_assumed_ids)
        // };
        let maps = vec![(SpaceSubtype::Main, &self.main_assumed_ids), (SpaceSubtype::Import, &self.imports_assumed_ids), (SpaceSubtype::Export, &self.exports_assumed_ids), (SpaceSubtype::Alias, &self.alias_assumed_ids), (SpaceSubtype::Components, &self.components_assumed_ids)];

        for (subty, map) in maps.iter() {
            for (idx, assumed) in map.iter() {
                if *assumed == assumed_id {
                    return Some((*subty, *idx));
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

            ComponentSection::Module |
            ComponentSection::CoreType |
            ComponentSection::ComponentType |
            ComponentSection::CoreInstance |
            ComponentSection::ComponentInstance |
            ComponentSection::Canon |
            ComponentSection::CustomSection |
            ComponentSection::ComponentStartSection => &mut self.main_assumed_ids
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
    Main
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ExternalItemKind {
    // Component-level spaces
    CompFunc,
    CompVal,
    CompType,
    CompInst,
    Comp,

    // Core space (added by component model)
    CoreInst,
    Module,

    // Core spaces that exist at the component-level
    CoreType,
    CoreFunc,
    CoreTable,
    CoreMemory,
    CoreGlobal,
    CoreTag,

    // Does not impact an index space
    NA,
}

impl From<&ComponentTypeRef> for ExternalItemKind {
    fn from(value: &ComponentTypeRef) -> Self {
        match value {
            ComponentTypeRef::Module(_) => Self::Module,
            ComponentTypeRef::Func(_) => Self::CompFunc,
            ComponentTypeRef::Type(_) => Self::CompType,
            ComponentTypeRef::Instance(_) => Self::CompInst,
            ComponentTypeRef::Component(_) => Self::CompInst,
            ComponentTypeRef::Value(_) => Self::CompVal,
        }
    }
}
impl From<&ExternalKind> for ExternalItemKind {
    fn from(value: &ExternalKind) -> Self {
        match value {
            ExternalKind::Func => ExternalItemKind::CoreFunc,
            ExternalKind::Table => ExternalItemKind::CoreTable,
            ExternalKind::Memory => ExternalItemKind::CoreMemory,
            ExternalKind::Global => ExternalItemKind::CoreGlobal,
            ExternalKind::Tag => ExternalItemKind::CoreTag,
            ExternalKind::FuncExact => ExternalItemKind::CoreFunc,
        }
    }
}
impl From<&ComponentExternalKind> for ExternalItemKind {
    fn from(value: &ComponentExternalKind) -> Self {
        match value {
            ComponentExternalKind::Module => Self::Module,
            ComponentExternalKind::Func => Self::CompFunc,
            ComponentExternalKind::Value => Self::CompVal,
            ComponentExternalKind::Type => Self::CompType,
            ComponentExternalKind::Instance => Self::CompInst,
            ComponentExternalKind::Component => Self::Comp
        }
    }
}
impl From<&Option<ComponentTypeRef>> for ExternalItemKind {
    fn from(value: &Option<ComponentTypeRef>) -> Self {
        if let Some(value) = value {
            Self::from(value)
        } else {
            Self::NA
        }
    }
}
impl From<&ComponentAlias<'_>> for ExternalItemKind {
    fn from(value: &ComponentAlias) -> Self {
        match value {
            ComponentAlias::InstanceExport { kind, .. } => match kind {
                ComponentExternalKind::Module => Self::Module,
                ComponentExternalKind::Func => {
                    Self::CompFunc
                },
                ComponentExternalKind::Value => Self::CompVal,
                ComponentExternalKind::Type => {
                    Self::CompType
                },
                ComponentExternalKind::Instance => Self::CompInst,
                ComponentExternalKind::Component => Self::Comp
            },
            ComponentAlias::Outer { kind, .. } => match kind {
                ComponentOuterAliasKind::CoreModule => Self::Module,
                ComponentOuterAliasKind::CoreType => Self::CoreType,
                ComponentOuterAliasKind::Type => Self::CompType,
                ComponentOuterAliasKind::Component => Self::Comp
            },
            ComponentAlias::CoreInstanceExport { kind, .. } => {
                match kind {
                    ExternalKind::Func => Self::CoreFunc,
                    ExternalKind::Table => Self::CoreTable,
                    ExternalKind::Memory => Self::CoreMemory,
                    ExternalKind::Global => Self::CoreGlobal,
                    ExternalKind::Tag => Self::CoreTag,
                    ExternalKind::FuncExact => Self::CoreFunc,
                }
            }
        }
    }
}
impl From<&CanonicalFunction> for ExternalItemKind {
    fn from(value: &CanonicalFunction) -> Self {
        match value {
            CanonicalFunction::Lift { .. } => Self::CompFunc,
            CanonicalFunction::Lower { .. } |
            CanonicalFunction::ResourceNew { .. } |
            CanonicalFunction::ResourceDrop { .. } |
            CanonicalFunction::ResourceDropAsync { .. } |
            CanonicalFunction::ResourceRep { .. } |
            CanonicalFunction::ThreadSpawnRef { .. } |
            CanonicalFunction::ThreadSpawnIndirect { .. } |
            CanonicalFunction::ThreadAvailableParallelism |
            CanonicalFunction::BackpressureSet |
            CanonicalFunction::TaskReturn { .. } |
            CanonicalFunction::TaskCancel |
            CanonicalFunction::ContextGet(_) |
            CanonicalFunction::ContextSet(_) |
            CanonicalFunction::SubtaskDrop |
            CanonicalFunction::SubtaskCancel { .. } |
            CanonicalFunction::StreamNew { .. } |
            CanonicalFunction::StreamRead { .. } |
            CanonicalFunction::StreamWrite { .. } |
            CanonicalFunction::StreamCancelRead { .. } |
            CanonicalFunction::StreamCancelWrite { .. } |
            CanonicalFunction::StreamDropReadable { .. } |
            CanonicalFunction::StreamDropWritable { .. } |
            CanonicalFunction::FutureNew { .. } |
            CanonicalFunction::FutureRead { .. } |
            CanonicalFunction::FutureWrite { .. } |
            CanonicalFunction::FutureCancelRead { .. } |
            CanonicalFunction::FutureCancelWrite { .. } |
            CanonicalFunction::FutureDropReadable { .. } |
            CanonicalFunction::FutureDropWritable { .. } |
            CanonicalFunction::ErrorContextNew { .. } |
            CanonicalFunction::ErrorContextDebugMessage { .. } |
            CanonicalFunction::ErrorContextDrop |
            CanonicalFunction::WaitableSetNew |
            CanonicalFunction::WaitableSetWait { .. } |
            CanonicalFunction::WaitableSetPoll { .. } |
            CanonicalFunction::WaitableSetDrop |
            CanonicalFunction::WaitableJoin => Self::CoreFunc,
            CanonicalFunction::BackpressureInc |
            CanonicalFunction::BackpressureDec |
            CanonicalFunction::ThreadYield { .. } |
            CanonicalFunction::ThreadIndex |
            CanonicalFunction::ThreadNewIndirect { .. } |
            CanonicalFunction::ThreadSwitchTo { .. } |
            CanonicalFunction::ThreadSuspend { .. } |
            CanonicalFunction::ThreadResumeLater |
            CanonicalFunction::ThreadYieldTo { .. } => todo!()
        }
    }
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
            Self::Func(_) |
            Self::Type(_) => Space::CompType,
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
            ComponentTypeRef::Module(_) => Space::CoreModule
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

            // Resource-related functions reference a resource type
            CanonicalFunction::ResourceNew { .. }
            | CanonicalFunction::ResourceDrop { .. }
            | CanonicalFunction::ResourceDropAsync { .. }
            | CanonicalFunction::ResourceRep { .. } => Space::CompFunc,

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
            CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_) => Space::CompFunc,

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
            InstantiationArgKind::Instance => Space::CoreInst
        }
    }
}

impl IndexSpaceOf for ExternalKind {
    fn index_space_of(&self) -> Space {
        match self {
            ExternalKind::Func => Space::CompFunc,
            ExternalKind::Table => Space::CoreTable,
            ExternalKind::Memory => Space::CoreMemory,
            ExternalKind::Global => Space::CoreGlobal,
            ExternalKind::Tag => Space::CoreTag,
            ExternalKind::FuncExact => Space::CompFunc,
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
            ComponentOuterAliasKind::Component => Space::CompInst,
        }
    }
}

/// To unify how I look up the referenced indices inside an IR node
pub trait ReferencedIndices {
    fn referenced_indices(&self) -> Option<Refs>;
}

#[derive(Default)]
pub struct Refs {
    pub comp: Option<IndexedRef>,
    pub inst: Option<IndexedRef>,
    pub module: Option<IndexedRef>,
    pub func: Option<IndexedRef>,
    pub ty: Option<IndexedRef>,
    pub mem: Option<IndexedRef>,
    pub table: Option<IndexedRef>,
    pub misc: Option<IndexedRef>,
    pub others: Vec<Option<Refs>>,
}
impl Refs {
    pub fn as_list(&self) -> Vec<IndexedRef> {
        let mut res = vec![];
        let Refs { comp, inst, module, func, ty, mem, table, misc, others } = self;

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
        if let Some(mem) = mem {
            res.push(*mem);
        }
        if let Some(table) = table {
            res.push(*table);
        }
        if let Some(misc) = misc {
            res.push(*misc);
        }
        others.iter().for_each(|o| {
            if let Some(o) = o {
                res.extend(o.as_list());
            }
        });

        res
    }
}

/// A single referenced index with semantic metadata
#[derive(Copy, Clone, Debug)]
pub struct IndexedRef {
    pub space: Space,
    pub index: u32,
}

impl ReferencedIndices for ComponentDefinedType<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            ComponentDefinedType::Record(records) => {
                let mut others = vec![];
                for (_, ty) in records.iter() {
                    others.push(ty.referenced_indices());
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            },
            ComponentDefinedType::Variant(variants) => {
                // Explanation of variants.refines:
                // This case `refines` (is a subtype/specialization of) another case in the same variant.
                // So the u32 refers to: the index of another case within the current variant’s case list.
                // It is NOT an index into some global index space (hence not handling it here)
                let mut others = vec![];
                for VariantCase { name: _, ty, refines: _ } in variants.iter() {
                    if let Some(t) = ty {
                        let ty_refs: Option<Refs> = t.referenced_indices();
                        others.push(ty_refs);
                    }
                }
                Some(Refs {
                    others,
                    ..Default::default()
                })
            },
            ComponentDefinedType::List(ty)
            | ComponentDefinedType::FixedSizeList(ty, _)
            | ComponentDefinedType::Option(ty) => ty.referenced_indices(),
            ComponentDefinedType::Tuple(tys) => {
                let mut others = vec![];
                for ty in tys.iter() {
                    others.push(ty.referenced_indices());
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
                let ok_r = if let Some(ok) = ok {
                    ok.referenced_indices()
                } else {
                    None
                };
                let err_r = if let Some(err) = err {
                    err.referenced_indices()
                } else {
                    None
                };
                Some(Refs {
                    others: vec![ ok_r, err_r ],
                    ..Default::default()
                })
            }
            ComponentDefinedType::Own(ty)
            | ComponentDefinedType::Borrow(ty) => Some(Refs {
                ty: Some(IndexedRef { space: Space::CompType, index: *ty}),
                ..Default::default()
            }),
            ComponentDefinedType::Future(ty)
            | ComponentDefinedType::Stream(ty) => if let Some(ty) = ty {
                ty.referenced_indices()
            } else {
            None
            }
        }
    }
}

impl ReferencedIndices for CanonicalFunction {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            CanonicalFunction::Lift { core_func_index, type_index, options } => {
                let mut others = vec![];
                // Recursively include indices from options
                for opt in options.iter() {
                    others.push(opt.referenced_indices());
                }
                Some(Refs {
                    func: Some(IndexedRef { space: Space::CoreFunc, index: *core_func_index }),
                    ty: Some(IndexedRef { space: Space::CompType, index: *type_index}),
                    others,
                    ..Default::default()
                })
            }

            CanonicalFunction::Lower { func_index, options } => {
                let mut others = vec![];
                // Recursively include indices from options
                for opt in options.iter() {
                    others.push(opt.referenced_indices());
                }
                Some(Refs {
                    func: Some(IndexedRef { space: Space::CompFunc, index: *func_index }),
                    others,
                    ..Default::default()
                })
            }

            CanonicalFunction::ResourceNew { resource }
            | CanonicalFunction::ResourceDrop { resource }
            | CanonicalFunction::ResourceDropAsync { resource }
            | CanonicalFunction::ResourceRep { resource }=> Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *resource }),
                    ..Default::default()
                }),

            CanonicalFunction::ThreadSpawnIndirect {
                func_ty_index,
                table_index,
            } => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *func_ty_index }),
                    table: Some(IndexedRef { space: Space::CoreTable, index: *table_index }),
                    ..Default::default()
                }),

            // other variants...
            _ => todo!()
        }
    }
}

impl ReferencedIndices for CanonicalOption {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            CanonicalOption::Memory(id) => Some(
                Refs {
                    mem: Some(IndexedRef { space: Space::CoreMemory, index: *id }),
                    ..Default::default()
                }),
            CanonicalOption::Realloc(id)
            | CanonicalOption::PostReturn(id)
            | CanonicalOption::Callback(id) => Some(
                Refs {
                    func: Some(IndexedRef { space: Space::CoreFunc, index: *id }),
                    ..Default::default()
                }),
            CanonicalOption::CoreType(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CoreType, index: *id }),
                    ..Default::default()
                }),
            CanonicalOption::Async
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Gc
            | CanonicalOption::UTF8
            | CanonicalOption::UTF16 => None
        }
    }
}

impl ReferencedIndices for ComponentImport<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        self.ty.referenced_indices()
    }
}
impl ReferencedIndices for ComponentTypeRef {
    fn referenced_indices(&self) -> Option<Refs> {
        match &self {
            // The reference is to a core module type.
            // The index is expected to be core type index to a core module type.
            ComponentTypeRef::Module(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CoreType, index: *id }),
                    ..Default::default()
                }
            ),
            ComponentTypeRef::Func(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *id }),
                    ..Default::default()
                }
            ),
            ComponentTypeRef::Instance(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *id }),
                    ..Default::default()
                }
            ),
            ComponentTypeRef::Component(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *id }),
                    ..Default::default()
                }
            ),
            ComponentTypeRef::Value(ty) => ty.referenced_indices(),
            ComponentTypeRef::Type(_) => None
        }
    }
}

impl ReferencedIndices for ComponentValType {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            ComponentValType::Primitive(_) => None,
            ComponentValType::Type(id) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CompType, index: *id }),
                    ..Default::default()
                }
            ),
        }
    }
}

impl ReferencedIndices for ComponentInstantiationArg<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        Some(Refs {
            ty: Some(IndexedRef { space: self.kind.index_space_of(), index: self.index }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for ComponentExport<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef { space: self.kind.index_space_of(), index: self.index }),
            ty: if let Some(t) = &self.ty {
                t.referenced_indices()?.ty
            } else {
                None
            },
            ..Default::default()
        })
    }
}

impl ReferencedIndices for Export<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef { space: self.kind.index_space_of(), index: self.index }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for InstantiationArg<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        Some(Refs {
            misc: Some(IndexedRef { space: self.kind.index_space_of(), index: self.index }),
            ..Default::default()
        })
    }
}

impl ReferencedIndices for Instance<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            Instance::Instantiate { module_index, args } => {
                let mut others = vec![];
                // Recursively include indices from options
                for arg in args.iter() {
                    others.push(arg.referenced_indices());
                }
                Some(Refs {
                    module: Some(IndexedRef { space: Space::CoreModule, index: *module_index }),
                    others,
                    ..Default::default()
                })
            }
            Instance::FromExports(exports) => {
                let mut others = vec![];
                // Recursively include indices from options
                for exp in exports.iter() {
                    others.push(exp.referenced_indices());
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
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            TypeRef::Func(ty)
            | TypeRef::Tag(TagType { kind: _, func_type_idx: ty }) => Some(
                Refs {
                    ty: Some(IndexedRef { space: Space::CoreType, index: *ty }),
                    ..Default::default()
                }
            ),
            _ => None
        }
    }
}

impl ReferencedIndices for ComponentAlias<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        let space = self.index_space_of();
        match self {
            ComponentAlias::InstanceExport { instance_index, .. } => Some(
                Refs {
                    ty: Some(IndexedRef { space, index: *instance_index }),
                    ..Default::default()
                }
            ),
            ComponentAlias::CoreInstanceExport { instance_index, .. } => Some(
                Refs {
                    ty: Some(IndexedRef { space, index: *instance_index }),
                    ..Default::default()
                }
            ),
            ComponentAlias::Outer { index, .. } => Some(
                Refs {
                    misc: Some(IndexedRef { space, index: *index }),
                    ..Default::default()
                }
            ),
        }
    }
}

impl ReferencedIndices for ComponentInstance<'_> {
    fn referenced_indices(&self) -> Option<Refs> {
        match self {
            ComponentInstance::Instantiate {
                component_index,
                args
            } => {
                let mut others = vec![];
                // Recursively include indices from args
                for arg in args.iter() {
                    others.push(arg.referenced_indices());
                }

                Some(
                    Refs {
                        comp: Some(IndexedRef { space: Space::CompType, index: *component_index }),
                        others,
                        ..Default::default()
                    }
                )
            }

            ComponentInstance::FromExports(export) => {
                let mut others = vec![];
                // Recursively include indices from args
                for exp in export.iter() {
                    others.push(exp.referenced_indices());
                }

                if !others.is_empty() {
                    Some(
                        Refs {
                            others,
                            ..Default::default()
                        }
                    )
                } else {
                    None
                }
            }
        }
    }
}
