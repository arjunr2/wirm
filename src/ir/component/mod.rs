#![allow(clippy::mut_range_bound)] // see https://github.com/rust-lang/rust-clippy/issues/6072
//! Intermediate Representation of a wasm component.

use crate::encode::component::encode;
use crate::error::Error;
use crate::ir::component::alias::Aliases;
use crate::ir::component::canons::Canons;
use crate::ir::component::idx_spaces::{
    IndexSpaceOf, IndexStore, ReferencedIndices, Space, SpaceId, SpaceSubtype, StoreHandle,
};
use crate::ir::component::section::{
    populate_space_for_comp_ty, populate_space_for_core_ty, ComponentSection,
};
use crate::ir::component::types::ComponentTypes;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{
    AliasFuncId, AliasId, CanonicalFuncId, ComponentExportId, ComponentTypeFuncId, ComponentTypeId,
    ComponentTypeInstanceId, CoreInstanceId, FunctionID, GlobalID, ModuleID,
};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::types::CustomSections;
use crate::ir::wrappers::add_to_namemap;
use std::cell::RefCell;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentFuncType, ComponentImport,
    ComponentInstance, ComponentStartFunction, ComponentType, CoreType, Encoding, Instance,
    InstanceTypeDeclaration, Parser, Payload,
};

mod alias;
mod canons;
pub mod idx_spaces;
pub(crate) mod section;
mod types;

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    /// Nested Components
    pub components: Vec<Component<'a>>,
    /// Modules
    pub modules: Vec<Module<'a>>,
    /// Component Types
    pub component_types: ComponentTypes<'a>,
    /// Component Instances
    pub component_instance: Vec<ComponentInstance<'a>>,
    /// Canons
    pub canons: Canons,

    /// Alias
    pub alias: Aliases<'a>,
    /// Imports
    pub imports: Vec<ComponentImport<'a>>,
    /// Exports
    pub exports: Vec<ComponentExport<'a>>,

    /// Core Types
    pub core_types: Vec<CoreType<'a>>,
    /// Core Instances
    pub instances: Vec<Instance<'a>>,

    // Tracks the index spaces of this component.
    pub(crate) space_id: SpaceId,
    pub(crate) index_store: StoreHandle,

    /// Custom sections
    pub custom_sections: CustomSections<'a>,
    /// Component Start Section
    pub start_section: Vec<ComponentStartFunction>,
    /// Sections of the Component. Represented as (#num of occurrences of a section, type of section)
    pub sections: Vec<(u32, ComponentSection)>,
    num_sections: usize,

    // Names
    pub(crate) component_name: Option<String>,
    pub(crate) core_func_names: wasm_encoder::NameMap,
    pub(crate) global_names: wasm_encoder::NameMap,
    pub(crate) memory_names: wasm_encoder::NameMap,
    pub(crate) tag_names: wasm_encoder::NameMap,
    pub(crate) table_names: wasm_encoder::NameMap,
    pub(crate) module_names: wasm_encoder::NameMap,
    pub(crate) core_instances_names: wasm_encoder::NameMap,
    pub(crate) core_type_names: wasm_encoder::NameMap,
    pub(crate) type_names: wasm_encoder::NameMap,
    pub(crate) instance_names: wasm_encoder::NameMap,
    pub(crate) components_names: wasm_encoder::NameMap,
    pub(crate) func_names: wasm_encoder::NameMap,
    pub(crate) value_names: wasm_encoder::NameMap,
}

impl Default for Component<'_> {
    fn default() -> Self {
        Component::new()
    }
}

impl<'a> Component<'a> {
    /// Creates a new Empty Component
    pub fn new() -> Self {
        Self::default()
    }

    fn add_section(&mut self, space: Space, sect: ComponentSection, idx: usize) -> usize {
        // get and save off the assumed id
        let assumed_id =
            self.index_store
                .borrow_mut()
                .assign_assumed_id(&self.space_id, &space, &sect, idx);

        // add to section order list
        if self.sections[self.num_sections - 1].1 == sect {
            self.sections[self.num_sections - 1].0 += 1;
        } else {
            self.sections.push((1, sect));
        }

        println!("assumed: {:?}", assumed_id);
        assumed_id.unwrap_or_else(|| idx)
    }

    /// Add a Module to this Component.
    pub fn add_module(&mut self, module: Module<'a>) -> ModuleID {
        let idx = self.modules.len();
        let id = self.add_section(module.index_space_of(), ComponentSection::Module, idx);
        self.modules.push(module);

        ModuleID(id as u32)
    }

    /// Add a Global to this Component.
    pub fn add_globals(&mut self, global: Global, module_idx: ModuleID) -> GlobalID {
        self.modules[*module_idx as usize].globals.add(global)
    }

    pub fn add_import(&mut self, import: ComponentImport<'a>) -> u32 {
        let idx = self.imports.len();
        let id = self.add_section(
            import.index_space_of(),
            ComponentSection::ComponentImport,
            idx,
        );
        self.imports.push(import);

        id as u32
    }

    pub fn add_alias_func(&mut self, alias: ComponentAlias<'a>) -> (AliasFuncId, AliasId) {
        print!(
            "[add_alias_func] '{}', from instance {}, curr-len: {}, ",
            if let ComponentAlias::InstanceExport { name, .. }
            | ComponentAlias::CoreInstanceExport { name, .. } = &alias
            {
                name
            } else {
                "no-name"
            },
            if let ComponentAlias::InstanceExport { instance_index, .. }
            | ComponentAlias::CoreInstanceExport { instance_index, .. } = &alias
            {
                format!("{instance_index}")
            } else {
                "NA".to_string()
            },
            self.canons.items.len()
        );
        let space = alias.index_space_of();
        let (_item_id, alias_id) = self.alias.add(alias);
        let id = self.add_section(space, ComponentSection::Alias, *alias_id as usize);
        println!("   --> @{}", id);

        (AliasFuncId(id as u32), alias_id)
    }

    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        print!("[add_canon_func] {:?}", canon);
        let space = canon.index_space_of();
        let idx = self.canons.add(canon).1;
        let id = self.add_section(space, ComponentSection::Canon, *idx as usize);
        println!("   --> @{}", id);

        CanonicalFuncId(id as u32)
    }

    pub(crate) fn add_component_type(
        &mut self,
        component_ty: ComponentType<'a>,
    ) -> (u32, ComponentTypeId) {
        // Handle the index space of this node
        let id = if matches!(
            component_ty,
            ComponentType::Component(_) | ComponentType::Instance(_)
        ) {
            // TODO: If this is injected, I need to populate its index space by processing its contents!
            //       will look similar to what I did in the original parsing logic of the bytes :)
            Some(self.index_store.borrow_mut().new_scope());
            todo!()
        } else {
            None
        };

        let space = component_ty.index_space_of();
        let ids = self.component_types.add(component_ty);
        let id = self.add_section(space, ComponentSection::ComponentType(id), *ids.1 as usize);

        (id as u32, ids.1)
    }

    pub fn add_type_instance(
        &mut self,
        decls: Vec<InstanceTypeDeclaration<'a>>,
    ) -> (ComponentTypeInstanceId, ComponentTypeId) {
        let (ty_inst_id, ty_id) =
            self.add_component_type(ComponentType::Instance(decls.into_boxed_slice()));

        // almost account for aliased types!
        (ComponentTypeInstanceId(ty_inst_id), ty_id)
    }

    pub fn add_type_func(
        &mut self,
        ty: ComponentFuncType<'a>,
    ) -> (ComponentTypeFuncId, ComponentTypeId) {
        let (ty_inst_id, ty_id) = self.add_component_type(ComponentType::Func(ty));

        // almost account for aliased types!
        (ComponentTypeFuncId(ty_inst_id), ty_id)
    }

    pub fn add_core_instance(&mut self, instance: Instance<'a>) -> CoreInstanceId {
        let idx = self.instances.len();
        let id = self.add_section(
            instance.index_space_of(),
            ComponentSection::CoreInstance,
            idx,
        );
        self.instances.push(instance);
        println!("[add_core_instance] id: {id}");

        CoreInstanceId(id as u32)
    }

    fn add_to_sections(
        sections: &mut Vec<(u32, ComponentSection)>,
        new_sections: &Vec<ComponentSection>,
        num_sections: &mut usize,
        sections_added: u32,
    ) {
        // We can only collapse sections if the new sections don't have
        // inner index spaces associated with them.
        let mut can_collapse = true;
        for sect in new_sections.iter() {
            if !sect.space_id().is_none() {
                can_collapse = false;
                break;
            }
        }

        if can_collapse {
            if *num_sections > 0 && sections[*num_sections - 1].1 == *new_sections.last().unwrap() {
                sections[*num_sections - 1].0 += sections_added;
                return;
            }
        }
        // Cannot collapse these, add one at a time!
        for sect in new_sections.iter() {
            sections.push((1, *sect));
            *num_sections += 1;
        }
    }

    /// Parse a `Component` from a wasm binary.
    ///
    /// Set enable_multi_memory to `true` to support parsing modules using multiple memories.
    /// Set with_offsets to `true` to save opcode pc offset metadata during parsing
    /// (can be used to determine the static pc offset inside a function body of the start of any opcode).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let comp = Component::parse(&buff, false, false).unwrap();
    /// ```
    pub fn parse(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
    ) -> Result<Self, Error> {
        let parser = Parser::new(0);

        let mut store = IndexStore::default();
        let space_id = store.new_scope();
        Component::parse_comp(
            wasm,
            enable_multi_memory,
            with_offsets,
            parser,
            0,
            &mut vec![],
            space_id,
            Rc::new(RefCell::new(store)),
        )
    }

    fn parse_comp(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
        parser: Parser,
        start: usize,
        parent_stack: &mut Vec<Encoding>,
        space_id: SpaceId,
        store_handle: StoreHandle,
    ) -> Result<Self, Error> {
        let mut modules = vec![];
        let mut core_types = vec![];
        let mut component_types = vec![];
        let mut imports = vec![];
        let mut exports = vec![];
        let mut instances = vec![];
        let mut canons = vec![];
        let mut alias = vec![];
        let mut component_instance = vec![];
        let mut custom_sections = vec![];
        let mut sections = vec![];
        let mut num_sections: usize = 0;
        let mut components: Vec<Component> = vec![];
        let mut start_section = vec![];
        let mut stack = vec![];

        // Names
        let mut component_name: Option<String> = None;
        let mut core_func_names = wasm_encoder::NameMap::new();
        let mut global_names = wasm_encoder::NameMap::new();
        let mut tag_names = wasm_encoder::NameMap::new();
        let mut memory_names = wasm_encoder::NameMap::new();
        let mut table_names = wasm_encoder::NameMap::new();
        let mut module_names = wasm_encoder::NameMap::new();
        let mut core_instance_names = wasm_encoder::NameMap::new();
        let mut instance_names = wasm_encoder::NameMap::new();
        let mut components_names = wasm_encoder::NameMap::new();
        let mut func_names = wasm_encoder::NameMap::new();
        let mut value_names = wasm_encoder::NameMap::new();
        let mut core_type_names = wasm_encoder::NameMap::new();
        let mut type_names = wasm_encoder::NameMap::new();
        //
        // let space_id = {
        //     let mut store = store_handle.borrow_mut();
        //     store.new_scope()
        // };

        for payload in parser.parse_all(wasm) {
            let payload = payload?;
            if let Payload::End(..) = payload {
                if !stack.is_empty() {
                    stack.pop();
                }
            }
            if !stack.is_empty() {
                continue;
            }
            match payload {
                Payload::ComponentImportSection(import_section_reader) => {
                    let temp: &mut Vec<ComponentImport> = &mut import_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentImport; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        imports.len(),
                        &new_sections,
                    );
                    imports.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentExportSection(export_section_reader) => {
                    let temp: &mut Vec<ComponentExport> = &mut export_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentExport; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        exports.len(),
                        &new_sections,
                    );
                    exports.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::InstanceSection(instance_section_reader) => {
                    let temp: &mut Vec<Instance> = &mut instance_section_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentExport; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        instances.len(),
                        &new_sections,
                    );
                    instances.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::CoreTypeSection(core_type_reader) => {
                    let temp: &mut Vec<CoreType> =
                        &mut core_type_reader.into_iter().collect::<Result<_, _>>()?;

                    let mut new_sects = vec![];
                    for ty in temp.iter() {
                        let section = populate_space_for_core_ty(ty, store_handle.clone());
                        new_sects.push(section)
                    }

                    let l = temp.len();
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        core_types.len(),
                        &new_sects,
                    );
                    core_types.append(temp);
                    Self::add_to_sections(&mut sections, &new_sects, &mut num_sections, l as u32);
                }
                Payload::ComponentTypeSection(component_type_reader) => {
                    let temp: &mut Vec<ComponentType> = &mut component_type_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;

                    let mut new_sects = vec![];
                    for ty in temp.iter() {
                        // CREATES A NEW IDX SPACE SCOPE (if Type::Component or Type::Instance)
                        let section = populate_space_for_comp_ty(ty, store_handle.clone());
                        new_sects.push(section);
                    }

                    let l = temp.len();
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        component_types.len(),
                        &new_sects,
                    );
                    component_types.append(temp);
                    Self::add_to_sections(&mut sections, &new_sects, &mut num_sections, l as u32);
                }
                Payload::ComponentInstanceSection(component_instances) => {
                    let temp: &mut Vec<ComponentInstance> =
                        &mut component_instances.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::ComponentInstance; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        component_instance.len(),
                        &new_sections,
                    );
                    component_instance.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentAliasSection(alias_reader) => {
                    let temp: &mut Vec<ComponentAlias> =
                        &mut alias_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::Alias; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        alias.len(),
                        &new_sections,
                    );
                    alias.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentCanonicalSection(canon_reader) => {
                    let temp: &mut Vec<CanonicalFunction> =
                        &mut canon_reader.into_iter().collect::<Result<_, _>>()?;
                    let l = temp.len();
                    let new_sections = vec![ComponentSection::Canon; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        canons.len(),
                        &new_sections,
                    );
                    canons.append(temp);
                    Self::add_to_sections(
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ModuleSection {
                    parser,
                    unchecked_range,
                } => {
                    // Indicating the start of a new module
                    parent_stack.push(Encoding::Module);
                    stack.push(Encoding::Module);
                    let m = Module::parse_internal(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        with_offsets,
                        parser,
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &m.index_space_of(),
                        &ComponentSection::Module,
                        modules.len(),
                    );
                    modules.push(m);
                    Self::add_to_sections(
                        &mut sections,
                        &vec![ComponentSection::Module],
                        &mut num_sections,
                        1,
                    );
                }
                Payload::ComponentSection {
                    parser,
                    unchecked_range,
                } => {
                    // Indicating the start of a new component

                    // CREATES A NEW IDX SPACE SCOPE
                    // TODO: This guy's index space is actually populated implicitly by the parse.
                    //       I just need to make sure that the way this works is compatible with the
                    //       new architecture.
                    let sub_space_id = store_handle.borrow_mut().new_scope();
                    let sect = ComponentSection::Component(sub_space_id);

                    parent_stack.push(Encoding::Component);
                    stack.push(Encoding::Component);
                    let cmp = Component::parse_comp(
                        &wasm[unchecked_range.start - start..unchecked_range.end - start],
                        enable_multi_memory,
                        with_offsets,
                        parser,
                        unchecked_range.start,
                        &mut stack,
                        sub_space_id,
                        Rc::clone(&store_handle),
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &cmp.index_space_of(),
                        &sect,
                        components.len(),
                    );
                    components.push(cmp);
                    Self::add_to_sections(&mut sections, &vec![sect], &mut num_sections, 1);
                }
                Payload::ComponentStartSection { start, range: _ } => {
                    start_section.push(start);
                    Self::add_to_sections(
                        &mut sections,
                        &vec![ComponentSection::ComponentStartSection],
                        &mut num_sections,
                        1,
                    );
                }
                Payload::CustomSection(custom_section_reader) => {
                    match custom_section_reader.as_known() {
                        wasmparser::KnownCustom::ComponentName(name_section_reader) => {
                            for subsection in name_section_reader {
                                #[allow(clippy::single_match)]
                                match subsection? {
                                    wasmparser::ComponentName::Component { name, .. } => {
                                        component_name = Some(name.parse().unwrap())
                                    }
                                    wasmparser::ComponentName::CoreFuncs(names) => {
                                        add_to_namemap(&mut core_func_names, names);
                                    }
                                    wasmparser::ComponentName::CoreGlobals(names) => {
                                        add_to_namemap(&mut global_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTables(names) => {
                                        add_to_namemap(&mut table_names, names);
                                    }
                                    wasmparser::ComponentName::CoreModules(names) => {
                                        add_to_namemap(&mut module_names, names);
                                    }
                                    wasmparser::ComponentName::CoreInstances(names) => {
                                        add_to_namemap(&mut core_instance_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTypes(names) => {
                                        add_to_namemap(&mut core_type_names, names);
                                    }
                                    wasmparser::ComponentName::Types(names) => {
                                        add_to_namemap(&mut type_names, names);
                                    }
                                    wasmparser::ComponentName::Instances(names) => {
                                        add_to_namemap(&mut instance_names, names);
                                    }
                                    wasmparser::ComponentName::Components(names) => {
                                        add_to_namemap(&mut components_names, names);
                                    }
                                    wasmparser::ComponentName::Funcs(names) => {
                                        add_to_namemap(&mut func_names, names);
                                    }
                                    wasmparser::ComponentName::Values(names) => {
                                        add_to_namemap(&mut value_names, names);
                                    }
                                    wasmparser::ComponentName::CoreMemories(names) => {
                                        add_to_namemap(&mut memory_names, names);
                                    }
                                    wasmparser::ComponentName::CoreTags(names) => {
                                        add_to_namemap(&mut tag_names, names);
                                    }
                                    wasmparser::ComponentName::Unknown { .. } => {}
                                }
                            }
                        }
                        _ => {
                            custom_sections
                                .push((custom_section_reader.name(), custom_section_reader.data()));
                            Self::add_to_sections(
                                &mut sections,
                                &vec![ComponentSection::CustomSection],
                                &mut num_sections,
                                1,
                            );
                        }
                    }
                }
                Payload::UnknownSection {
                    id,
                    contents: _,
                    range: _,
                } => return Err(Error::UnknownSection { section_id: id }),
                Payload::Version { .. } | Payload::End { .. } => {} // nothing to do
                other => println!("TODO: Not sure what to do for: {:?}", other),
            }
        }

        Ok(Component {
            modules,
            alias: Aliases::new(alias),
            core_types,
            component_types: ComponentTypes::new(component_types),
            imports,
            exports,
            instances,
            component_instance,
            canons: Canons::new(canons),
            space_id,
            index_store: store_handle,
            custom_sections: CustomSections::new(custom_sections),
            sections,
            start_section,
            num_sections,
            component_name,
            core_func_names,
            global_names,
            memory_names,
            tag_names,
            table_names,
            module_names,
            core_instances_names: core_instance_names,
            core_type_names,
            type_names,
            instance_names,
            components_names,
            func_names,
            components,
            value_names,
        })
    }

    /// Encode a `Component` to bytes.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use wirm::Component;
    ///
    /// let file = "path_to_file";
    /// let buff = wat::parse_file(file).expect("couldn't convert the input wat to Wasm");
    /// let mut comp = Component::parse(&buff, false, false).unwrap();
    /// let result = comp.encode();
    /// ```
    pub fn encode(&mut self) -> Vec<u8> {
        encode(&self)
    }

    pub fn get_type_of_exported_lift_func(
        &self,
        export_id: ComponentExportId,
    ) -> Option<&ComponentType<'a>> {
        let store = self.index_store.borrow();
        if let Some(export) = self.exports.get(*export_id as usize) {
            println!(
                "[get_type_of_exported_func] @{} export: {:?}",
                *export_id, export
            );
            if let Some(refs) = export.referenced_indices() {
                let list = refs.as_list();
                assert_eq!(1, list.len());

                let (vec, f_idx) = store.index_from_assumed_id(&self.space_id, &list[0]);
                let func = match vec {
                    SpaceSubtype::Export | SpaceSubtype::Components | SpaceSubtype::Import => {
                        unreachable!()
                    }
                    SpaceSubtype::Alias => {
                        self.alias.items.get(f_idx).unwrap().referenced_indices()
                    }
                    SpaceSubtype::Main => {
                        self.canons.items.get(f_idx).unwrap().referenced_indices()
                    }
                };
                if let Some(func_refs) = func {
                    let (ty, t_idx) = store.index_from_assumed_id(&self.space_id, func_refs.ty());
                    if !matches!(ty, SpaceSubtype::Main) {
                        panic!("Should've been an main space!")
                    }

                    let res = self.component_types.items.get(t_idx);
                    res
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Print a rudimentary textual representation of a `Component`
    pub fn print(&self) {
        // Print Alias
        if !self.alias.items.is_empty() {
            eprintln!("Alias Section:");
            for alias in self.alias.items.iter() {
                print_alias(alias);
            }
            eprintln!();
        }

        // Print CoreType
        if !self.core_types.is_empty() {
            eprintln!("Core Type Section:");
            for cty in self.core_types.iter() {
                print_core_type(cty);
            }
            eprintln!();
        }

        // Print ComponentType
        if !self.component_types.items.is_empty() {
            eprintln!("Component Type Section:");
            for cty in self.component_types.items.iter() {
                print_component_type(cty);
            }
            eprintln!();
        }

        // Print Imports
        if !self.imports.is_empty() {
            eprintln!("Imports Section:");
            for imp in self.imports.iter() {
                print_component_import(imp);
            }
            eprintln!();
        }

        // Print Exports
        if !self.imports.is_empty() {
            eprintln!("Exports Section:");
            for exp in self.exports.iter() {
                print_component_export(exp);
            }
            eprintln!();
        }
    }

    /// Emit the Component into a wasm binary file.
    pub fn emit_wasm(&mut self, file_name: &str) -> Result<(), std::io::Error> {
        let wasm = self.encode();
        std::fs::write(file_name, wasm)?;
        Ok(())
    }

    /// Get Local Function ID by name
    // Note: returned absolute id here
    pub fn get_fid_by_name(&self, name: &str, module_idx: ModuleID) -> Option<FunctionID> {
        for (idx, func) in self.modules[*module_idx as usize]
            .functions
            .iter()
            .enumerate()
        {
            if let FuncKind::Local(l) = &func.kind {
                if let Some(n) = &l.body.name {
                    if n == name {
                        return Some(FunctionID(idx as u32));
                    }
                }
            }
        }
        None
    }
}
