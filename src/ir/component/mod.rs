#![allow(clippy::mut_range_bound)] // see https://github.com/rust-lang/rust-clippy/issues/6072
//! Intermediate Representation of a wasm component.

use crate::assert_registered_with_id;
use crate::encode::component::encode;
use crate::error::Error;
use crate::ir::component::alias::Aliases;
use crate::ir::component::canons::Canons;
use crate::ir::component::idx_spaces::{
    Depth, IndexSpaceOf, IndexStore, ReferencedIndices, Space, SpaceId, SpaceSubtype, StoreHandle,
};
use crate::ir::component::scopes::{IndexScopeRegistry, RegistryHandle};
use crate::ir::component::section::{
    get_sections_for_comp_ty, get_sections_for_core_ty_and_assign_top_level_ids,
    populate_space_for_comp_ty, populate_space_for_core_ty, ComponentSection,
};
use crate::ir::component::types::ComponentTypes;
use crate::ir::helpers::{
    print_alias, print_component_export, print_component_import, print_component_type,
    print_core_type,
};
use crate::ir::id::{
    AliasFuncId, AliasId, CanonicalFuncId, ComponentExportId, ComponentId, ComponentTypeFuncId,
    ComponentTypeId, ComponentTypeInstanceId, CoreInstanceId, FunctionID, GlobalID, ModuleID,
};
use crate::ir::module::module_functions::FuncKind;
use crate::ir::module::module_globals::Global;
use crate::ir::module::Module;
use crate::ir::types::CustomSections;
use crate::ir::wrappers::add_to_namemap;
use std::cell::RefCell;
use std::ops::Deref;
use std::rc::Rc;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentFuncType, ComponentImport,
    ComponentInstance, ComponentStartFunction, ComponentType, CoreType, Encoding, Instance,
    InstanceTypeDeclaration, Parser, Payload,
};

mod alias;
mod canons;
pub mod idx_spaces;
pub mod scopes;
pub(crate) mod section;
mod types;

/// A stable handle identifying a parsed WebAssembly component.
///
/// `ComponentHandle` represents the identity of a component across parsing,
/// instrumentation, and encoding phases. It exists to support advanced
/// instrumentation and encoding logic where component identity must remain
/// stable even if the component value itself is moved or wrapped by the user.
///
/// Most users will not need to interact with this type directly; it is primarily
/// used by APIs that perform structural transformations or encoding.
///
/// The handle does **not** grant ownership or mutation access to the component.
/// It exists solely to preserve identity across phases.
#[derive(Clone, Debug)]
pub struct ComponentHandle<'a> {
    // TODO: Maybe I can just override scope lookups for components using a saved
    //       component ID on the IR node? Like, that's the only one with the diff
    //       behavior? I _think_ that'd let me avoid this ComponentHandle wrapper
    //       nonsense that's mucking up the public API.
    inner: Rc<Component<'a>>,
}
impl<'a> ComponentHandle<'a> {
    pub fn new(inner: Rc<Component<'a>>) -> Self {
        Self { inner }
    }

    /// Emit the Component into a wasm binary file.
    pub fn emit_wasm(&mut self, file_name: &str) -> Result<(), std::io::Error> {
        let wasm = self.encode();
        std::fs::write(file_name, wasm)?;
        Ok(())
    }

    pub fn encode(&self) -> Vec<u8> {
        assert_registered_with_id!(self.inner.scope_registry, &*self.inner, self.inner.space_id);
        self.inner.encode()
    }

    /// Mutably access the entire underlying [`Component`] in a controlled scope.
    ///
    /// This is the lowest-level mutation API on [`ComponentHandle`]. It grants
    /// temporary, exclusive mutable access to the underlying component and applies
    /// the provided closure to it.
    ///
    /// ## Why this exists
    ///
    /// The component is internally reference-counted to support stable identity
    /// (used for scope registration and lookup). As a result, direct mutable access
    /// is only possible when the component is uniquely owned.
    ///
    /// This method:
    ///
    /// * Enforces **exclusive ownership** at the time of mutation
    /// * Prevents mutable references from escaping the call
    /// * Centralizes the ownership check in one place
    ///
    /// Higher-level helpers such as [`mut_module_at`] and [`mut_component_at`] are
    /// built on top of this pattern and should be preferred when possible.
    ///
    /// ## Panics
    ///
    /// Panics if the component is shared (i.e. there is more than one owner).
    /// Instrumentation requires exclusive access.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // Rename the component
    /// handle.mutate(|comp| {
    ///     comp.component_name = Some("instrumented".into());
    /// });
    /// ```
    ///
    /// ## Notes
    ///
    /// * The mutable borrow of the component is limited to the duration of the closure.
    /// * Do not store references to the component outside the closure.
    /// * Prefer more specific mutation helpers when available.
    pub fn mutate<F, R>(&mut self, f: F) -> R
    where
        F: for<'b> FnOnce(&'b mut Component<'a>) -> R,
    {
        let comp =
            Rc::get_mut(&mut self.inner).expect("Cannot mutably access Component: it is shared");
        f(comp)
    }

    /// Mutably access a specific core module within this component.
    ///
    /// This method provides scoped mutable access to a single [`Module`] identified
    /// by index, without exposing the rest of the component to mutation.
    ///
    /// ## Why this exists
    ///
    /// Many instrumentation passes operate at the module level. This helper:
    ///
    /// * Avoids borrowing the entire component mutably
    /// * Prevents accidental cross-module mutation
    /// * Keeps mutations localized and easier to reason about
    ///
    /// Like all mutation APIs on [`ComponentHandle`], access is only permitted when
    /// the component is uniquely owned.
    ///
    /// ## Panics
    ///
    /// Panics if the component is shared or if `idx` is out of bounds.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // Add a `nop` instruction to start of the first module's first function.
    /// handle.mut_module_at(0, |module| {
    ///     module.functions.get_mut(0).unwrap_local().add_instr(
    ///         Operator::Nop,
    ///         0
    ///     );
    /// });
    /// ```
    ///
    /// ## Notes
    ///
    /// * The module reference cannot escape the closure.
    /// * If you need to mutate multiple modules, call this method multiple times.
    /// * For structural changes to the component itself, use [`mutate`].
    pub fn mut_module_at<F, R>(&mut self, idx: usize, f: F) -> R
    where
        F: FnOnce(&mut Module) -> R,
    {
        let comp =
            Rc::get_mut(&mut self.inner).expect("Cannot mutably access Component: it is shared");
        f(&mut comp.modules[idx])
    }

    /// Mutably access a nested component within this component.
    ///
    /// This method provides scoped mutable access to an inner [`ComponentHandle`]
    /// by index, enabling recursive instrumentation of nested components.
    ///
    /// ## Why this exists
    ///
    /// Components may contain other components, each with their own index spaces
    /// and scopes. This helper:
    ///
    /// * Preserves component identity and scope registration
    /// * Enables safe, recursive traversal and mutation
    /// * Avoids exposing raw mutable access to internal structures
    ///
    /// Each nested component is still subject to the same ownership rules as the
    /// outer component.
    ///
    /// ## Panics
    ///
    /// Panics if the component is shared or if `idx` is out of bounds.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // Instrument a nested component
    /// handle.mut_component_at(0, |child| {
    ///     child.mutate(|comp| {
    ///         comp.component_name = Some("child".into());
    ///     });
    /// });
    /// ```
    ///
    /// ## Notes
    ///
    /// * The nested component is accessed through its own [`ComponentHandle`].
    /// * This preserves invariants around scope registration and index spaces.
    /// * Mutations must remain within the closure.
    pub fn mut_component_at<F, R>(&mut self, idx: usize, f: F) -> R
    where
        F: FnOnce(&mut ComponentHandle) -> R,
    {
        let comp =
            Rc::get_mut(&mut self.inner).expect("Cannot mutably access Component: it is shared");
        f(&mut comp.components[idx])
    }

    /// Mutably access a single instance within this component and apply a scoped mutation.
    ///
    /// This method provides controlled mutable access to an [`Instance`] without exposing
    /// long-lived mutable borrows of the underlying [`Component`]. The mutation is performed
    /// by invoking the provided closure on the selected instance.
    ///
    /// ## Why this API exists
    ///
    /// Instances inside a component may contain references with lifetimes tied to the
    /// component itself (for example, borrowed strings parsed from the original binary).
    /// Allowing callers to obtain `&mut Instance` directly would permit those references
    /// to escape, which is unsound and rejected by the Rust compiler.
    ///
    /// To prevent this, the closure is required to be valid for **any** borrow lifetime.
    /// This guarantees that:
    ///
    /// * The mutable reference to the instance **cannot escape** the closure
    /// * The instance **cannot be stored or returned**
    /// * All mutations are strictly **local and scoped**
    ///
    /// This pattern is intentionally used to support safe IR instrumentation while
    /// preserving internal invariants.
    ///
    /// ## Panics
    ///
    /// Panics if the underlying component is shared (i.e. if there are multiple owners).
    /// Instrumentation requires exclusive access to the component.
    ///
    /// ## Example
    ///
    /// ```rust,ignore
    /// // Append an instantiation argument to a specific instance
    /// wasm.mut_instance_at(0, |inst| {
    ///     if let Instance::Instantiate { args, .. } = inst {
    ///         args.push(InstantiationArg {
    ///             name: "my_lib",
    ///             kind: InstantiationArgKind::Instance,
    ///             index: 3,
    ///         });
    ///     }
    /// });
    /// ```
    ///
    /// ## Notes for users
    ///
    /// * You cannot return the instance or store references to it outside the closure.
    /// * This is by design and enforced at compile time.
    /// * If you need to perform multiple mutations, do so within the same closure.
    ///
    /// This API is part of the library’s commitment to **safe, structured instrumentation**
    /// of component IR without relying on runtime borrow checking or unsafe code.
    pub fn mut_instance_at<F>(&mut self, i: usize, f: F)
    where
        F: for<'b> FnOnce(&'b mut Instance<'a>),
    {
        let comp =
            Rc::get_mut(&mut self.inner).expect("Cannot mutably access Component: it is shared");

        f(&mut comp.instances[i]);
    }
}
impl<'a> Deref for ComponentHandle<'a> {
    type Target = Component<'a>;
    fn deref(&self) -> &Component<'a> {
        &self.inner
    }
}

#[derive(Debug)]
/// Intermediate Representation of a wasm component.
pub struct Component<'a> {
    pub id: ComponentId,
    /// Nested Components
    pub components: Vec<ComponentHandle<'a>>,
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
    pub(crate) space_id: SpaceId, // cached for quick lookup!
    pub(crate) scope_registry: RegistryHandle,
    pub(crate) index_store: StoreHandle,

    /// Custom sections
    pub custom_sections: CustomSections<'a>,
    /// Component Start Section
    pub start_section: Vec<ComponentStartFunction>,
    /// Sections of the Component. Represented as (#num of occurrences of a section, type of section)
    pub sections: Vec<(u32, ComponentSection)>,
    num_sections: usize,

    // pub interned_strs: Vec<Box<str>>,

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

impl<'a> Component<'a> {
    /// Creates a new Empty Component
    pub fn new(component: Self) -> ComponentHandle<'a> {
        ComponentHandle::new(Rc::new(component))
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
        let space = alias.index_space_of();
        let (_item_id, alias_id) = self.alias.add(alias);
        let id = self.add_section(space, ComponentSection::Alias, *alias_id as usize);

        (AliasFuncId(id as u32), alias_id)
    }

    pub fn add_canon_func(&mut self, canon: CanonicalFunction) -> CanonicalFuncId {
        let space = canon.index_space_of();
        let idx = self.canons.add(canon).1;
        let id = self.add_section(space, ComponentSection::Canon, *idx as usize);

        CanonicalFuncId(id as u32)
    }

    pub(crate) fn add_component_type(
        &mut self,
        component_ty: ComponentType<'a>,
    ) -> (u32, ComponentTypeId) {
        let space = component_ty.index_space_of();
        let ids = self.component_types.add(component_ty);
        let id = self.add_section(space, ComponentSection::ComponentType, *ids.1 as usize);

        // Handle the index space of this node
        populate_space_for_comp_ty(
            self.component_types.items.last().unwrap(),
            self.scope_registry.clone(),
            self.index_store.clone(),
        );

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

        CoreInstanceId(id as u32)
    }

    fn add_to_sections(
        has_subscope: bool,
        sections: &mut Vec<(u32, ComponentSection)>,
        new_sections: &Vec<ComponentSection>,
        num_sections: &mut usize,
        sections_added: u32,
    ) {
        // We can only collapse sections if the new sections don't have
        // inner index spaces associated with them.
        let can_collapse = !has_subscope;

        if can_collapse {
            if *num_sections > 0 && sections[*num_sections - 1].1 == *new_sections.last().unwrap() {
                sections[*num_sections - 1].0 += sections_added;
                return;
            }
        }
        // Cannot collapse these, add one at a time!
        for sect in new_sections.iter() {
            sections.push((1, sect.clone()));
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
        wasm: &'_ [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
    ) -> Result<ComponentHandle<'_>, Error> {
        let parser = Parser::new(0);

        let registry = IndexScopeRegistry::default();
        let mut store = IndexStore::default();
        let space_id = store.new_scope();
        let mut next_comp_id = 0;
        let res = Component::parse_comp(
            wasm,
            enable_multi_memory,
            with_offsets,
            parser,
            0,
            &mut vec![],
            space_id,
            Rc::new(RefCell::new(registry)),
            Rc::new(RefCell::new(store)),
            &mut next_comp_id,
        );
        //
        // if let Ok(comp) = &res {
        //     comp.scope_registry.borrow_mut().register(&comp.inner, space_id, ScopeOwnerKind::Component);
        //     assert_eq!(comp.space_id, comp.scope_registry.borrow().scope_entry(&comp.inner).unwrap().space);
        // }
        res
    }

    fn parse_comp(
        wasm: &'a [u8],
        enable_multi_memory: bool,
        with_offsets: bool,
        parser: Parser,
        start: usize,
        parent_stack: &mut Vec<Encoding>,
        space_id: SpaceId,
        registry_handle: RegistryHandle,
        store_handle: StoreHandle,
        next_comp_id: &mut u32,
    ) -> Result<ComponentHandle<'a>, Error> {
        let my_comp_id = ComponentId(*next_comp_id);
        *next_comp_id += 1;

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
        let mut components: Vec<ComponentHandle> = vec![];
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
                        false,
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
                        false,
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
                    let new_sections = vec![ComponentSection::CoreInstance; l];
                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &temp,
                        instances.len(),
                        &new_sections,
                    );
                    instances.append(temp);
                    Self::add_to_sections(
                        false,
                        &mut sections,
                        &new_sections,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::CoreTypeSection(core_type_reader) => {
                    let temp: &mut Vec<CoreType> =
                        &mut core_type_reader.into_iter().collect::<Result<_, _>>()?;

                    let old_len = core_types.len();
                    let l = temp.len();
                    core_types.append(temp);

                    let mut new_sects = vec![];
                    let mut has_subscope = false;
                    for (idx, ty) in core_types[old_len..].iter().enumerate() {
                        let (new_sect, sect_has_subscope) =
                            get_sections_for_core_ty_and_assign_top_level_ids(
                                ty,
                                old_len + idx,
                                &space_id,
                                store_handle.clone(),
                            );
                        has_subscope |= sect_has_subscope;
                        new_sects.push(new_sect);
                    }

                    // TODO: Properly populate the index space for rec groups!
                    // store_handle.borrow_mut().assign_assumed_id_for(
                    //     &space_id,
                    //     &core_types[old_len..].to_vec(),
                    //     old_len,
                    //     &new_sects,
                    // );
                    Self::add_to_sections(
                        has_subscope,
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        l as u32,
                    );
                }
                Payload::ComponentTypeSection(component_type_reader) => {
                    let temp: &mut Vec<ComponentType> = &mut component_type_reader
                        .into_iter()
                        .collect::<Result<_, _>>()?;

                    let old_len = component_types.len();
                    let l = temp.len();
                    component_types.append(temp);

                    let mut new_sects = vec![];
                    let mut has_subscope = false;
                    for ty in &component_types[old_len..] {
                        let (new_sect, sect_has_subscope) = get_sections_for_comp_ty(ty);
                        has_subscope |= sect_has_subscope;
                        new_sects.push(new_sect);
                    }

                    store_handle.borrow_mut().assign_assumed_id_for(
                        &space_id,
                        &component_types[old_len..].to_vec(),
                        old_len,
                        &new_sects,
                    );
                    Self::add_to_sections(
                        has_subscope,
                        &mut sections,
                        &new_sects,
                        &mut num_sections,
                        l as u32,
                    );
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
                        false,
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
                        false,
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
                        false,
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
                        false,
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
                    let sub_space_id = store_handle.borrow_mut().new_scope();
                    let sect = ComponentSection::Component;

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
                        Rc::clone(&registry_handle),
                        Rc::clone(&store_handle),
                        next_comp_id,
                    )?;
                    store_handle.borrow_mut().assign_assumed_id(
                        &space_id,
                        &cmp.index_space_of(),
                        &sect,
                        components.len(),
                    );
                    components.push(cmp);

                    Self::add_to_sections(true, &mut sections, &vec![sect], &mut num_sections, 1);
                }
                Payload::ComponentStartSection { start, range: _ } => {
                    start_section.push(start);
                    Self::add_to_sections(
                        false,
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
                                false,
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

        // Scope discovery
        for comp in &components {
            let sub_space_id = comp.space_id;
            registry_handle.borrow_mut().register(comp, sub_space_id);
            assert_registered_with_id!(registry_handle, comp, sub_space_id);
        }
        for ty in &core_types {
            populate_space_for_core_ty(ty, registry_handle.clone(), store_handle.clone());
        }
        for ty in &component_types {
            populate_space_for_comp_ty(ty, registry_handle.clone(), store_handle.clone());
        }

        let comp_rc = Rc::new(Component {
            id: my_comp_id,
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
            scope_registry: registry_handle,
            index_store: store_handle,
            custom_sections: CustomSections::new(custom_sections),
            sections,
            start_section,
            num_sections,
            // interned_strs: vec![],
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
        });

        comp_rc
            .scope_registry
            .borrow_mut()
            .register(&*comp_rc, space_id);
        let handle = ComponentHandle::new(comp_rc);
        assert_eq!(
            handle.inner.space_id,
            handle
                .inner
                .scope_registry
                .borrow()
                .scope_entry(&*handle.inner)
                .unwrap()
                .space
        );

        Ok(handle)
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
    fn encode(&self) -> Vec<u8> {
        encode(&self)
    }

    pub fn get_type_of_exported_lift_func(
        &self,
        export_id: ComponentExportId,
    ) -> Option<&ComponentType<'a>> {
        let mut store = self.index_store.borrow_mut();
        if let Some(export) = self.exports.get(*export_id as usize) {
            if let Some(refs) = export.referenced_indices(Depth::default()) {
                let list = refs.as_list();
                assert_eq!(1, list.len());

                let (vec, f_idx, subidx) =
                    store.index_from_assumed_id_no_cache(&self.space_id, &list[0]);
                assert!(subidx.is_none(), "a lift function shouldn't reference anything with a subvec space (like a recgroup)");
                let func = match vec {
                    SpaceSubtype::Export | SpaceSubtype::Components | SpaceSubtype::Import => {
                        unreachable!()
                    }
                    SpaceSubtype::Alias => self
                        .alias
                        .items
                        .get(f_idx)
                        .unwrap()
                        .referenced_indices(Depth::default()),
                    SpaceSubtype::Main => self
                        .canons
                        .items
                        .get(f_idx)
                        .unwrap()
                        .referenced_indices(Depth::default()),
                };
                if let Some(func_refs) = func {
                    let (ty, t_idx, subidx) =
                        store.index_from_assumed_id(&self.space_id, func_refs.ty());
                    assert!(subidx.is_none(), "a lift function shouldn't reference anything with a subvec space (like a recgroup)");
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
