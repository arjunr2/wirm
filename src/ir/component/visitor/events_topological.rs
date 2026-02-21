use std::collections::HashSet;
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration, ModuleTypeDeclaration};
use crate::{Component, Module};
use crate::ir::component::idx_spaces::{IndexSpaceOf, Space, SpaceSubtype};
use crate::ir::component::refs::{Depth, RefKind, ReferencedIndices};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::VisitCtx;
use crate::ir::types::CustomSection;

pub(crate) fn get_topological_events<'ir>(
    component: &'ir Component<'ir>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>
) {
    let mut topo = TopoCtx::default();

    ctx.inner.push_component(component);
    ctx.inner.push_comp_section_tracker();
    out.push(VisitEvent::enter_root_comp(
        component
    ));

    topo.collect_component(component, None, ctx);

    out.push(VisitEvent::exit_root_comp(
        component
    ));
}

#[derive(Default)]
struct TopoCtx<'ir> {
    seen: HashSet<NodeKey>,
    events: Vec<VisitEvent<'ir>>
}
impl<'ir> TopoCtx<'ir> {
    fn collect_component(
        &mut self,
        comp: &'ir Component<'ir>,
        idx: Option<usize>,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::Component(id(comp));
        if !self.seen.insert(key) {
            return;
        }

        if let Some(idx) = idx {
            ctx.inner.push_comp_section_tracker();
            ctx.inner.push_component(comp);
            self.events.push(VisitEvent::enter_comp(idx, comp));
        }

        for (count, section) in comp.sections.iter() {
            let start_idx = ctx.inner.visit_section(section, *count as usize);
            self.collect_section_items(
                comp,
                section,
                start_idx,
                *count as usize,
                ctx,
            );
        }


        if let Some(idx) = idx {
            ctx.inner.pop_comp_section_tracker();
            ctx.inner.pop_component();
            self.events.push(VisitEvent::exit_comp(idx, comp));
        }
    }
    fn collect_module(
        &mut self,
        module: &'ir Module<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            module,
            ctx,
            NodeKey::Module(id(module)),
            |events| events.push(VisitEvent::module(
                module.index_space_of().into(), idx, module
            ))
        );
    }
    fn collect_type<T>(
        &mut self,
        node: &'ir T,
        key: NodeKey,
        ctx: &mut VisitCtx<'ir>,
        emit_enter: impl FnOnce(&mut Vec<VisitEvent<'ir>>),
        emit_exit: impl FnOnce(&mut Vec<VisitEvent<'ir>>),
        walk_body: impl FnOnce(&mut Self, &mut VisitCtx<'ir>),
    )
    where
        T: GetScopeKind + ReferencedIndices + 'ir,
    {
        if !self.seen.insert(key) {
            return;
        }

        // resolve referenced indices first
        ctx.inner.maybe_enter_scope(node);
        self.collect_deps(node, ctx);
        ctx.inner.maybe_exit_scope(node);

        // structured enter
        emit_enter(&mut self.events);

        // walk inner declarations
        walk_body(self, ctx);

        // structured exit
        emit_exit(&mut self.events);
    }

    fn collect_component_type(
        &mut self,
        node: &'ir ComponentType<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::ComponentType(id(node));

        self.collect_type(
            node,
            key,
            ctx,
            |events| {
                events.push(VisitEvent::enter_comp_type(
                    node.index_space_of().into(),
                    idx,
                    node
                ));
            },
            |events| {
                events.push(VisitEvent::exit_comp_type(
                    node.index_space_of().into(),
                    idx,
                    node
                ));
            },
            |this, ctx| {
                match node {
                    ComponentType::Component(decls) => {
                        for (i, decl) in decls.iter().enumerate() {
                            this.collect_component_type_decl(node, decl, i, ctx);
                        }
                    }

                    ComponentType::Instance(decls) => {
                        for (i, decl) in decls.iter().enumerate() {
                            this.collect_instance_type_decl(node, decl, i, ctx);
                        }
                    }

                    // no sub-scoping for the below variants
                    ComponentType::Defined(_) | ComponentType::Func(_) | ComponentType::Resource { .. } => {}
                }
            },
        );
    }
    fn collect_component_type_decl(
        &mut self,
        parent: &'ir ComponentType<'ir>,
        decl: &'ir ComponentTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            decl,
            ctx,
            NodeKey::ComponentTypeDecl(id(parent), idx),
            |events| events.push(VisitEvent::comp_type_decl(
                parent, idx, decl
            ))
        );
    }
    fn collect_instance_type_decl(
        &mut self,
        parent: &'ir ComponentType<'ir>,
        decl: &'ir InstanceTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            decl,
            ctx,
            // use the parent since this guy doesn't have global identity
            NodeKey::InstanceTypeDecl(id(parent), idx),
            |events| events.push(VisitEvent::inst_type_decl(
                parent, idx, decl
            ))
        );
    }
    fn collect_comp_inst(
        &mut self,
        inst: &'ir ComponentInstance<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            inst,
            ctx,
            NodeKey::ComponentInstance(id(inst)),
            |events| events.push(VisitEvent::comp_inst(
                inst.index_space_of().into(), idx, inst
            ))
        );
    }
    fn collect_core_inst(
        &mut self,
        inst: &'ir Instance<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            inst,
            ctx,
            NodeKey::CoreInst(id(inst)),
            |events| events.push(VisitEvent::core_inst(
                inst.index_space_of().into(), idx, inst
            ))
        );
    }

    fn collect_core_type(
        &mut self,
        node: &'ir CoreType<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::CoreType(id(node));

        self.collect_type(
            node,
            key,
            ctx,
            |events| {
                events.push(VisitEvent::enter_core_type(
                    node.index_space_of().into(),
                    idx,
                    node
                ));
            },
            |events| {
                events.push(VisitEvent::exit_core_type(
                    node.index_space_of().into(),
                    idx,
                    node
                ));
            },
            |this, ctx| {
                match node {
                    CoreType::Module(decls ) => {
                        for (i, decl) in decls.iter().enumerate() {
                            this.collect_module_type_decl(node, decl, i, ctx);
                        }
                    }

                    // no sub-scoping for the below variant
                    CoreType::Rec(_) => {}
                }
            },
        );
    }
    fn collect_module_type_decl(
        &mut self,
        parent: &'ir CoreType<'ir>,
        decl: &'ir ModuleTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            decl,
            ctx,
            NodeKey::ModuleTypeDecl(id(parent), idx),
            |events| events.push(VisitEvent::mod_type_decl(
                parent, idx, decl
            ))
        );
    }
    fn collect_canon(
        &mut self,
        canon: &'ir CanonicalFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            canon,
            ctx,
            NodeKey::Canon(id(canon)),
            |events| events.push(VisitEvent::canon(
                canon.index_space_of().into(), idx, canon
            ))
        );
    }
    fn collect_export(
        &mut self,
        export: &'ir ComponentExport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            export,
            ctx,
            NodeKey::Export(id(export)),
            |events| events.push(VisitEvent::export(
                export.index_space_of().into(), idx, export
            ))
        );
    }
    fn collect_import(
        &mut self,
        import: &'ir ComponentImport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            import,
            ctx,
            NodeKey::Import(id(import)),
            |events| events.push(VisitEvent::import(
                import.index_space_of().into(), idx, import
            ))
        );
    }
    fn collect_alias(
        &mut self,
        alias: &'ir ComponentAlias<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            alias,
            ctx,
            NodeKey::Alias(id(alias)),
            |events| events.push(VisitEvent::alias(
                alias.index_space_of().into(), idx, alias
            ))
        );
    }
    fn collect_custom_section(
        &mut self,
        sect: &'ir CustomSection<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            sect,
            ctx,
            NodeKey::Custom(id(sect)),
            |events| events.push(VisitEvent::custom_sect(
                sect.index_space_of().into(), idx, sect
            ))
        );
    }
    fn collect_start_section(
        &mut self,
        func: &'ir ComponentStartFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_item(
            func,
            ctx,
            NodeKey::Start(id(func)),
            |events| events.push(VisitEvent::start_func(
                func.index_space_of().into(), idx, func
            ))
        );
    }

    fn collect_section_items(
        &mut self,
        comp: &'ir Component<'ir>,
        section: &ComponentSection,
        start_idx: usize,
        count: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        for i in 0..count {
            let idx = start_idx + i;

            match section {
                ComponentSection::Component =>
                    self.collect_component(&comp.components[idx], Some(idx), ctx),

                ComponentSection::Module =>
                    self.collect_module(&comp.modules[idx], idx, ctx),

                ComponentSection::ComponentType =>
                    self.collect_component_type(
                        &comp.component_types.items[idx], idx, ctx),

                ComponentSection::ComponentInstance =>
                    self.collect_comp_inst(
                        &comp.component_instance[idx], idx, ctx),

                ComponentSection::Canon =>
                    self.collect_canon(&comp.canons.items[idx], idx, ctx),

                ComponentSection::Alias =>
                    self.collect_alias(&comp.alias.items[idx], idx, ctx),

                ComponentSection::ComponentImport =>
                    self.collect_import(&comp.imports[idx], idx, ctx),

                ComponentSection::ComponentExport =>
                    self.collect_export(&comp.exports[idx], idx, ctx),

                ComponentSection::CoreType =>
                    self.collect_core_type(&comp.core_types[idx], idx, ctx),

                ComponentSection::CoreInstance =>
                    self.collect_core_inst(&comp.instances[idx], idx, ctx),

                ComponentSection::CustomSection =>
                    self.collect_custom_section(
                        &comp.custom_sections.custom_sections[idx], idx, ctx),

                ComponentSection::ComponentStartSection =>
                    self.collect_start_section(
                        &comp.start_section[idx], idx, ctx),
            }
        }
    }


    fn collect_item<N: 'ir>(
        &mut self,
        node: &'ir N,
        ctx: &mut VisitCtx<'ir>,
        key: NodeKey,
        emit: impl FnOnce(&mut Vec<VisitEvent<'ir>>),
    )
    where
        N: ReferencedIndices + GetScopeKind + 'ir,
    {
        if !self.seen.insert(key) {
            return;
        }

        ctx.inner.maybe_enter_scope(node);
        self.collect_deps(node, ctx);
        ctx.inner.maybe_exit_scope(node);

        emit(&mut self.events);
    }
    fn collect_deps<T: ReferencedIndices + 'ir>(
        &mut self,
        item: &'ir T,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let refs = item.referenced_indices(Depth::default());
        for RefKind { ref_, .. } in refs.iter() {
            let (vec, idx, subidx) = ctx.inner.index_from_assumed_id(ref_);
            if ref_.space != Space::CoreType {
                assert!(
                    subidx.is_none(),
                    "only core types (with rec groups) should ever have subvec indices!"
                );
            }

            let comp_id = ctx.inner.comp_at(ref_.depth);
            let referenced_comp = ctx.inner.comp_store.get(comp_id);

            let space = ref_.space;
            match vec {
                SpaceSubtype::Main => match space {
                    Space::Comp => self.collect_component(
                        &referenced_comp.components[idx],
                        Some(idx),
                        ctx
                    ),
                    Space::CompType => self.collect_component_type(
                        &referenced_comp.component_types.items[idx],
                        idx,
                        ctx
                    ),
                    Space::CompInst => self.collect_comp_inst(
                        &referenced_comp.component_instance[idx],
                        idx,
                        ctx
                    ),
                    Space::CoreInst => self.collect_core_inst(
                        &referenced_comp.instances[idx],
                        idx,
                        ctx
                    ),
                    Space::CoreModule => self.collect_module(
                        &referenced_comp.modules[idx],
                        idx,
                        ctx
                    ),
                    Space::CoreType => self.collect_core_type(
                        &referenced_comp.core_types[idx],
                        idx,
                        ctx
                    ),
                    Space::CompFunc | Space::CoreFunc => self.collect_canon(
                        &referenced_comp.canons.items[idx],
                        idx,
                        ctx
                    ),
                    Space::CompVal
                    | Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag
                    | Space::NA => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                },
                SpaceSubtype::Export => self.collect_export(
                    &referenced_comp.exports[idx],
                    idx,
                    ctx
                ),
                SpaceSubtype::Import => self.collect_import(
                    &referenced_comp.imports[idx],
                    idx,
                    ctx
                ),
                SpaceSubtype::Alias => self.collect_alias(
                    &referenced_comp.alias.items[idx],
                    idx,
                    ctx
                ),
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum NodeKey {
    Component(*const ()),
    Module(*const ()),
    ComponentType(*const ()),
    ComponentTypeDecl(*const (), usize), // parent ptr + index
    InstanceTypeDecl(*const (), usize),
    CoreType(*const ()),
    ModuleTypeDecl(*const (), usize),
    ComponentInstance(*const ()),
    CoreInst(*const ()),
    Alias(*const ()),
    Import(*const ()),
    Export(*const ()),
    Canon(*const ()),
    Custom(*const ()),
    Start(*const ()),
}

fn id<T>(ptr: &T) -> *const () {
    ptr as *const T as *const ()
}

