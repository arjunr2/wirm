use crate::ir::component::idx_spaces::{IndexSpaceOf, Space, SpaceSubtype};
use crate::ir::component::refs::{RefKind, ReferencedIndices};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::VisitCtx;
use crate::ir::types::CustomSection;
use crate::{Component, Module};
use std::collections::HashSet;
use wasmparser::{
    CanonicalFunction, ComponentAlias, ComponentExport, ComponentImport, ComponentInstance,
    ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance,
    InstanceTypeDeclaration, ModuleTypeDeclaration,
};

pub(crate) fn get_topological_events<'ir>(
    component: &'ir Component<'ir>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    let mut topo = TopoCtx::default();

    ctx.inner.push_component(component);
    out.push(VisitEvent::enter_root_comp(component));

    topo.collect_component(component, None, ctx);
    out.extend(topo.events);

    out.push(VisitEvent::exit_root_comp(component));
    ctx.inner.pop_component();
}

#[derive(Default)]
struct TopoCtx<'ir> {
    seen: HashSet<NodeKey>,
    events: Vec<VisitEvent<'ir>>,
}
impl<'ir> TopoCtx<'ir> {
    fn collect_component(
        &mut self,
        comp: &'ir Component<'ir>,
        idx: Option<usize>,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::Component(id(comp));
        if !self.visit_once(key) {
            return;
        }

        if let Some(idx) = idx {
            ctx.inner.push_component(comp);
            self.events.push(VisitEvent::enter_comp(idx, comp));
        }

        for (count, section) in comp.sections.iter() {
            let start_idx = ctx.inner.visit_section(section, *count as usize);
            self.collect_section_items(comp, section, start_idx, *count as usize, ctx);
        }

        if let Some(idx) = idx {
            ctx.inner.pop_component();
            self.events.push(VisitEvent::exit_comp(idx, comp));
        }
    }
    fn collect_module(&mut self, module: &'ir Module<'ir>, idx: usize, ctx: &mut VisitCtx<'ir>) {
        self.collect_node(
            module,
            NodeKey::Module(id(module)),
            ctx,
            None,
            VisitEvent::module(module.index_space_of().into(), idx, module),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_component_type(
        &mut self,
        node: &'ir ComponentType<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        let key = NodeKey::ComponentType(id(node));

        self.collect_node(
            node,
            key,
            ctx,
            Some(VisitEvent::enter_comp_type(
                node.index_space_of().into(),
                idx,
                node,
            )),
            VisitEvent::exit_comp_type(node.index_space_of().into(), idx, node),
            |this, node, ctx| {
                match node {
                    ComponentType::Component(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::component_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_component_type_decl(node, item, i, cx);
                                },
                                ctx,
                            );
                        }
                    }

                    ComponentType::Instance(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::inst_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_instance_type_decl(node, item, i, cx);
                                },
                                ctx,
                            );
                        }
                    }

                    // no sub-scoping for the below variants
                    ComponentType::Defined(_)
                    | ComponentType::Func(_)
                    | ComponentType::Resource { .. } => {}
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
        self.events
            .push(VisitEvent::comp_type_decl(parent, idx, decl));
        match decl {
            ComponentTypeDeclaration::Type(ty) => self.collect_component_type(ty, idx, ctx),
            ComponentTypeDeclaration::CoreType(ty) => self.collect_core_type(ty, idx, ctx),
            ComponentTypeDeclaration::Alias(_)
            | ComponentTypeDeclaration::Export { .. }
            | ComponentTypeDeclaration::Import(_) => {}
        }
    }
    fn collect_instance_type_decl(
        &mut self,
        parent: &'ir ComponentType<'ir>,
        decl: &'ir InstanceTypeDeclaration<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.events
            .push(VisitEvent::inst_type_decl(parent, idx, decl));
        match decl {
            InstanceTypeDeclaration::Type(ty) => self.collect_component_type(ty, idx, ctx),
            InstanceTypeDeclaration::CoreType(ty) => self.collect_core_type(ty, idx, ctx),
            InstanceTypeDeclaration::Alias(_) | InstanceTypeDeclaration::Export { .. } => {}
        }
    }
    fn collect_comp_inst(
        &mut self,
        inst: &'ir ComponentInstance<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            inst,
            NodeKey::ComponentInstance(id(inst)),
            ctx,
            None,
            VisitEvent::comp_inst(inst.index_space_of().into(), idx, inst),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_core_inst(&mut self, inst: &'ir Instance<'ir>, idx: usize, ctx: &mut VisitCtx<'ir>) {
        self.collect_node(
            inst,
            NodeKey::CoreInst(id(inst)),
            ctx,
            None,
            VisitEvent::core_inst(inst.index_space_of().into(), idx, inst),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }

    fn collect_core_type(&mut self, node: &'ir CoreType<'ir>, idx: usize, ctx: &mut VisitCtx<'ir>) {
        let key = NodeKey::CoreType(id(node));

        let (enter_evt, exit_evt) = if let CoreType::Rec(group) = node {
            (
                VisitEvent::enter_rec_group(group.types().len(), node),
                VisitEvent::exit_rec_group(),
            )
        } else {
            (
                VisitEvent::enter_core_type(node.index_space_of().into(), idx, node),
                VisitEvent::exit_core_type(node.index_space_of().into(), idx, node),
            )
        };

        self.collect_node(
            node,
            key,
            ctx,
            Some(enter_evt),
            exit_evt,
            |this, node, ctx| {
                match node {
                    CoreType::Module(decls) => {
                        for (i, item) in decls.iter().enumerate() {
                            this.collect_subitem(
                                decls,
                                item,
                                i,
                                NodeKey::module_type_decl,
                                |inner_this, item, i, cx| {
                                    inner_this.collect_module_type_decl(node, item, i, cx);
                                },
                                ctx,
                            );
                        }
                    }

                    // no sub-scoping for the below variant
                    CoreType::Rec(group) => {
                        for (subvec_idx, item) in group.types().enumerate() {
                            this.events
                                .push(VisitEvent::core_subtype(idx, subvec_idx, item));
                        }
                    }
                }
            },
        );
    }
    fn collect_module_type_decl(
        &mut self,
        parent: &'ir CoreType<'ir>,
        decl: &'ir ModuleTypeDeclaration<'ir>,
        idx: usize,
        _: &mut VisitCtx<'ir>,
    ) {
        self.events
            .push(VisitEvent::mod_type_decl(parent, idx, decl))
    }
    fn collect_canon(
        &mut self,
        canon: &'ir CanonicalFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            canon,
            NodeKey::Canon(id(canon)),
            ctx,
            None,
            VisitEvent::canon(canon.index_space_of().into(), idx, canon),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_export(
        &mut self,
        export: &'ir ComponentExport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            export,
            NodeKey::Export(id(export)),
            ctx,
            None,
            VisitEvent::export(export.index_space_of().into(), idx, export),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_import(
        &mut self,
        import: &'ir ComponentImport<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            import,
            NodeKey::Import(id(import)),
            ctx,
            None,
            VisitEvent::import(import.index_space_of().into(), idx, import),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_alias(
        &mut self,
        alias: &'ir ComponentAlias<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            alias,
            NodeKey::Alias(id(alias)),
            ctx,
            None,
            VisitEvent::alias(alias.index_space_of().into(), idx, alias),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_custom_section(
        &mut self,
        sect: &'ir CustomSection<'ir>,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            sect,
            NodeKey::Custom(id(sect)),
            ctx,
            None,
            VisitEvent::custom_sect(sect.index_space_of().into(), idx, sect),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
        );
    }
    fn collect_start_section(
        &mut self,
        func: &'ir ComponentStartFunction,
        idx: usize,
        ctx: &mut VisitCtx<'ir>,
    ) {
        self.collect_node(
            func,
            NodeKey::Start(id(func)),
            ctx,
            None,
            VisitEvent::start_func(func.index_space_of().into(), idx, func),
            |this, node, cx| {
                this.collect_deps(node, cx);
            },
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
                ComponentSection::Component => {
                    self.collect_component(&comp.components[idx], Some(idx), ctx)
                }

                ComponentSection::Module => self.collect_module(&comp.modules[idx], idx, ctx),

                ComponentSection::ComponentType => {
                    self.collect_component_type(&comp.component_types.items[idx], idx, ctx)
                }

                ComponentSection::ComponentInstance => {
                    self.collect_comp_inst(&comp.component_instance[idx], idx, ctx)
                }

                ComponentSection::Canon => self.collect_canon(&comp.canons.items[idx], idx, ctx),

                ComponentSection::Alias => self.collect_alias(&comp.alias.items[idx], idx, ctx),

                ComponentSection::ComponentImport => {
                    self.collect_import(&comp.imports[idx], idx, ctx)
                }

                ComponentSection::ComponentExport => {
                    self.collect_export(&comp.exports[idx], idx, ctx)
                }

                ComponentSection::CoreType => {
                    self.collect_core_type(&comp.core_types[idx], idx, ctx)
                }

                ComponentSection::CoreInstance => {
                    self.collect_core_inst(&comp.instances[idx], idx, ctx)
                }

                ComponentSection::CustomSection => self.collect_custom_section(
                    &comp.custom_sections.custom_sections[idx],
                    idx,
                    ctx,
                ),

                ComponentSection::ComponentStartSection => {
                    self.collect_start_section(&comp.start_section[idx], idx, ctx)
                }
            }
        }
    }

    fn collect_node<T>(
        &mut self,
        node: &'ir T,
        key: NodeKey,
        ctx: &mut VisitCtx<'ir>,
        enter_event: Option<VisitEvent<'ir>>,
        exit_event: VisitEvent<'ir>,
        walk: impl FnOnce(&mut Self, &'ir T, &mut VisitCtx<'ir>),
    ) where
        T: GetScopeKind + ReferencedIndices + 'ir,
    {
        if !self.visit_once(key) {
            return;
        }

        if let Some(evt) = enter_event {
            self.events.push(evt)
        }

        // walk inner declarations
        ctx.inner.maybe_enter_scope(node);
        walk(self, node, ctx);
        ctx.inner.maybe_exit_scope(node);

        self.events.push(exit_event);
    }
    fn collect_deps<T: ReferencedIndices + 'ir>(&mut self, item: &'ir T, ctx: &mut VisitCtx<'ir>) {
        let refs = item.referenced_indices();
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
                    Space::Comp => {
                        self.collect_component(&referenced_comp.components[idx], Some(idx), ctx)
                    }
                    Space::CompType => self.collect_component_type(
                        &referenced_comp.component_types.items[idx],
                        idx,
                        ctx,
                    ),
                    Space::CompInst => {
                        self.collect_comp_inst(&referenced_comp.component_instance[idx], idx, ctx)
                    }
                    Space::CoreInst => {
                        self.collect_core_inst(&referenced_comp.instances[idx], idx, ctx)
                    }
                    Space::CoreModule => {
                        self.collect_module(&referenced_comp.modules[idx], idx, ctx)
                    }
                    Space::CoreType => {
                        self.collect_core_type(&referenced_comp.core_types[idx], idx, ctx)
                    }
                    Space::CompFunc | Space::CoreFunc => {
                        self.collect_canon(&referenced_comp.canons.items[idx], idx, ctx)
                    }
                    Space::CompVal
                    | Space::CoreMemory
                    | Space::CoreTable
                    | Space::CoreGlobal
                    | Space::CoreTag
                    | Space::NA => unreachable!(
                        "This spaces don't exist in a main vector on the component IR: {vec:?}"
                    ),
                },
                SpaceSubtype::Export => {
                    self.collect_export(&referenced_comp.exports[idx], idx, ctx)
                }
                SpaceSubtype::Import => {
                    self.collect_import(&referenced_comp.imports[idx], idx, ctx)
                }
                SpaceSubtype::Alias => {
                    self.collect_alias(&referenced_comp.alias.items[idx], idx, ctx)
                }
            }
        }
    }

    fn collect_subitem<T: ReferencedIndices + GetScopeKind + 'ir>(
        &mut self,
        all: &'ir [T],
        item: &'ir T,
        item_idx: usize,
        gen_key: fn(&T, usize) -> NodeKey,
        mut emit_item: impl FnMut(&mut Self, &'ir T, usize, &mut VisitCtx<'ir>),
        ctx: &mut VisitCtx<'ir>,
    ) {
        if !self.visit_once(gen_key(item, item_idx)) {
            return;
        }

        // collect the dependencies of this guy
        ctx.inner.maybe_enter_scope(item);
        let refs = item.referenced_indices();
        for RefKind { ref_, .. } in refs.iter() {
            if !ref_.depth.is_curr() {
                continue;
            }
            let (vec, idx, ..) = ctx.inner.index_from_assumed_id(ref_);
            assert_eq!(vec, SpaceSubtype::Main);
            let dep_item = &all[idx];

            if !self.visit_once(gen_key(dep_item, idx)) {
                continue;
            }

            // collect subitem
            emit_item(self, dep_item, idx, ctx);
        }

        ctx.inner.maybe_exit_scope(item);

        // collect item
        emit_item(self, item, item_idx, ctx);
    }
    fn visit_once(&mut self, key: NodeKey) -> bool {
        self.seen.insert(key)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum NodeKey {
    Component(*const ()),
    Module(*const ()),
    ComponentType(*const ()),
    ComponentTypeDecl(*const (), usize), // decl ptr + index
    InstanceTypeDecl(*const (), usize),  // decl ptr + index
    CoreType(*const ()),
    ModuleTypeDecl(*const (), usize), // decl ptr + index
    ComponentInstance(*const ()),
    CoreInst(*const ()),
    Alias(*const ()),
    Import(*const ()),
    Export(*const ()),
    Canon(*const ()),
    Custom(*const ()),
    Start(*const ()),
}
impl NodeKey {
    fn inst_type_decl(decl: &InstanceTypeDeclaration, idx: usize) -> Self {
        Self::InstanceTypeDecl(id(decl), idx)
    }
    fn component_type_decl(decl: &ComponentTypeDeclaration, idx: usize) -> Self {
        Self::ComponentTypeDecl(id(decl), idx)
    }
    fn module_type_decl(decl: &ModuleTypeDeclaration, idx: usize) -> Self {
        Self::ModuleTypeDecl(id(decl), idx)
    }
}

fn id<T>(ptr: &T) -> *const () {
    ptr as *const T as *const ()
}
