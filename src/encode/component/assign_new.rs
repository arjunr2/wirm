use std::collections::HashMap;
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration, ModuleTypeDeclaration, SubType};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::{IndexSpaceOf, ScopeId, Space};
use crate::ir::component::refs::IndexedRef;
use crate::ir::component::visitor::{walk_topological, ComponentVisitor, ItemKind, VisitCtx};
use crate::ir::component::visitor::utils::ScopeStack;

pub(crate) fn assign_indices(component: &Component) -> ActualIds {
    let mut assigner = Assigner::default();
    // TODO: Just pull the event vector to keep from generating 2x
    walk_topological(component, &mut assigner);

    assigner.ids
}

#[derive(Default)]
struct Assigner {
    ids: ActualIds
}
impl Assigner {
    fn assign_actual_id(&mut self, cx: &VisitCtx<'_>, space: &Space, assumed_id: u32) {
        let curr_scope = cx.inner.scope_stack.curr_space_id();
        self.ids.assign_actual_id(curr_scope, space, assumed_id as usize)
    }
}
impl ComponentVisitor<'_> for Assigner {
    fn exit_component(&mut self, cx: &VisitCtx<'_>, id: u32, component: &Component<'_>) {
        self.assign_actual_id(cx, &component.index_space_of(), id)
    }
    fn visit_module(&mut self, cx: &VisitCtx<'_>, id: u32, module: &Module<'_>) {
        self.assign_actual_id(cx, &module.index_space_of(), id)
    }
    fn visit_comp_type_decl(&mut self, cx: &VisitCtx<'_>, _decl_idx: usize, id: u32, _parent: &ComponentType<'_>, decl: &ComponentTypeDeclaration<'_>) {
        self.assign_actual_id(cx, &decl.index_space_of(), id)
    }
    fn visit_inst_type_decl(&mut self, cx: &VisitCtx<'_>, _decl_idx: usize, id: u32, _parent: &ComponentType<'_>, decl: &InstanceTypeDeclaration<'_>) {
        self.assign_actual_id(cx, &decl.index_space_of(), id)
    }
    fn exit_comp_type(&mut self, cx: &VisitCtx<'_>, id: u32, ty: &ComponentType<'_>) {
        self.assign_actual_id(cx, &ty.index_space_of(), id)
    }
    fn visit_comp_instance(&mut self, cx: &VisitCtx<'_>, id: u32, instance: &ComponentInstance<'_>) {
        self.assign_actual_id(cx, &instance.index_space_of(), id)
    }
    fn visit_canon(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, id: u32, canon: &CanonicalFunction) {
        self.assign_actual_id(cx, &canon.index_space_of(), id)
    }
    fn visit_alias(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, id: u32, alias: &ComponentAlias<'_>) {
        self.assign_actual_id(cx, &alias.index_space_of(), id)
    }
    fn visit_comp_import(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, id: u32, import: &ComponentImport<'_>) {
        self.assign_actual_id(cx, &import.index_space_of(), id)
    }
    fn visit_comp_export(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, id: u32, export: &ComponentExport<'_>) {
        self.assign_actual_id(cx, &export.index_space_of(), id)
    }
    fn visit_module_type_decl(&mut self, cx: &VisitCtx<'_>, _decl_idx: usize, id: u32, _parent: &CoreType<'_>, decl: &ModuleTypeDeclaration<'_>) {
        self.assign_actual_id(cx, &decl.index_space_of(), id)
    }
    fn enter_core_rec_group(&mut self, cx: &VisitCtx<'_>, _count: usize, _core_type: &CoreType<'_>) {
        // just need to make sure there's a scope built :)
        // this is relevant for: (component (core rec) )
        self.ids.add_scope(cx.inner.scope_stack.curr_space_id());
    }
    fn visit_core_subtype(&mut self, cx: &VisitCtx<'_>, id: u32, subtype: &SubType) {
        self.assign_actual_id(cx, &subtype.index_space_of(), id)
    }
    fn exit_core_type(&mut self, cx: &VisitCtx<'_>, id: u32, core_type: &CoreType<'_>) {
        self.assign_actual_id(cx, &core_type.index_space_of(), id)
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'_>, id: u32, inst: &Instance<'_>) {
        self.assign_actual_id(cx, &inst.index_space_of(), id)
    }
}

#[derive(Clone, Default)]
pub struct ActualIds {
    scopes: HashMap<ScopeId, IdsForScope>
}
impl ActualIds {
    pub fn add_scope(&mut self, id: ScopeId) {
        self.scopes.entry(id).or_default();
    }
    pub fn get_scope(&self, id: ScopeId) -> &IdsForScope {
        self.scopes.get(&id).unwrap_or_else(|| {
            panic!("Could not find assigned IDs for scope with ID: {id}");
        })
    }
    pub fn assign_actual_id(&mut self, id: ScopeId, space: &Space, assumed_id: usize) {
        let ids = self.scopes.entry(id).or_default();
        ids.assign_actual_id(space, assumed_id)
    }
    pub fn lookup_actual_id_or_panic(&self, scope_stack: &ScopeStack, r: &IndexedRef) -> usize {
        let scope_id = scope_stack.space_at_depth(&r.depth);
        let ids = self.scopes.get(&scope_id).unwrap_or_else(|| {
            panic!("Attempted to assign a non-existent scope: {scope_id}");
        });
        ids.lookup_actual_id_or_panic(r)
    }
}

/// This is used at encode time. It tracks the actual ID that has been assigned
/// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
/// This is important since we know what ID should be associated with something only at encode time,
/// since instrumentation has finished at that point and encoding of component items
/// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
#[derive(Clone, Default)]
pub struct IdsForScope {
    scope_id: ScopeId,

    // Component-level spaces
    pub comp: IdTracker,
    pub comp_func: IdTracker,
    pub comp_val: IdTracker,
    pub comp_type: IdTracker,
    pub comp_inst: IdTracker,

    // Core space (added by component model)
    pub core_inst: IdTracker, // (these are module instances)
    pub module: IdTracker,

    // Core spaces that exist at the component-level
    pub core_type: IdTracker,
    pub core_func: IdTracker, // these are canonical function decls!
    pub core_memory: IdTracker,
    pub core_table: IdTracker,
    pub core_global: IdTracker,
    pub core_tag: IdTracker,
}
impl IdsForScope {
    pub fn assign_actual_id(&mut self, space: &Space, assumed_id: usize) {
        if let Some(space) = self.get_space_mut(space) {
            space.assign_actual_id(assumed_id);
        }
    }

    fn get_space_mut(&mut self, space: &Space) -> Option<&mut IdTracker> {
        let s = match space {
            Space::Comp => &mut self.comp,
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
            Space::NA => return None,
        };
        Some(s)
    }

    fn get_space(&self, space: &Space) -> Option<&IdTracker> {
        let s = match space {
            Space::Comp => &self.comp,
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
            Space::NA => return None,
        };
        Some(s)
    }

    pub(crate) fn lookup_actual_id_or_panic(&self, r: &IndexedRef) -> usize {
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
}

#[derive(Clone, Default)]
struct IdTracker {
    /// This is used at encode time. It tracks the actual ID that has been assigned
    /// to some item by allowing for lookup of the assumed ID: `assumed_id -> actual_id`
    /// This is important since we know what ID should be associated with something only at encode time,
    /// since instrumentation has finished at that point and encoding of component items
    /// can be done out-of-order to satisfy possible forward-references injected during instrumentation.
    actual_ids: HashMap<usize, usize>,

    /// This is the current ID that we've reached associated with this index space.
    current_id: usize,
}
impl IdTracker {
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

    pub fn lookup_actual_id(&self, id: usize) -> Option<&usize> {
        self.actual_ids.get(&id)
    }
}
