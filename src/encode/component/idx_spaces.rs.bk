use std::collections::HashMap;
use wasmparser::CanonicalFunction;
use crate::Component;

pub(crate) enum Space {
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
}

#[derive(Debug, Default)]
pub(crate) struct IndexSpaces {
    // Component-level spaces
    pub comp_func: HashMap<u32, u32>,   // original_id -> assigned_id
    pub comp_val: HashMap<u32, u32>,
    pub comp_type: HashMap<u32, u32>,
    pub comp_inst: HashMap<u32, u32>,
    pub comp: HashMap<u32, u32>,

    // Core space (added by component model)
    pub core_inst: HashMap<u32, u32>,
    pub module: HashMap<u32, u32>,

    // Core spaces that exist at the component-level
    pub core_type: HashMap<u32, u32>,
    pub core_func: HashMap<u32, u32>,
    pub core_table: HashMap<u32, u32>,
    pub core_memory: HashMap<u32, u32>,
    pub core_global: HashMap<u32, u32>,
    pub core_tag: HashMap<u32, u32>
}
impl IndexSpaces {
    pub(crate) fn get_space(&self, space: &Space) -> &HashMap<u32, u32> {
        match space {
            Space::CompFunc => &self.comp_func,
            Space::CompVal => &self.comp_val,
            Space::CompType => &self.comp_type,
            Space::CompInst => &self.comp_inst,
            Space::Comp => &self.comp,
            Space::CoreInst => &self.core_inst,
            Space::Module => &self.module,
            Space::CoreType => &self.core_type,
            Space::CoreFunc => &self.core_func,
            Space::CoreTable => &self.core_table,
            Space::CoreMemory => &self.core_memory,
            Space::CoreGlobal => &self.core_global,
            Space::CoreTag => &self.core_tag,
        }
    }
    pub(crate) fn get_space_mut(&mut self, space: &Space) -> &mut HashMap<u32, u32> {
        match space {
            Space::CompFunc => &mut self.comp_func,
            Space::CompVal => &mut self.comp_val,
            Space::CompType => &mut self.comp_type,
            Space::CompInst => &mut self.comp_inst,
            Space::Comp => &mut self.comp,
            Space::CoreInst => &mut self.core_inst,
            Space::Module => &mut self.module,
            Space::CoreType => &mut self.core_type,
            Space::CoreFunc => &mut self.core_func,
            Space::CoreTable => &mut self.core_table,
            Space::CoreMemory => &mut self.core_memory,
            Space::CoreGlobal => &mut self.core_global,
            Space::CoreTag => &mut self.core_tag,
        }
    }
}

pub(crate) trait IdxSpace {
    fn idx_space(&self) -> Space;
}

impl IdxSpace for Component<'_> {
    fn idx_space(&self) -> Space {
        Space::Comp
    }
}

impl IdxSpace for CanonicalFunction {
    fn idx_space(&self) -> Space {
        todo!()
    }
}
