use wasm_encoder::{Alias, ComponentAliasSection, ComponentCoreTypeEncoder, ComponentDefinedTypeEncoder, ComponentFuncTypeEncoder, ComponentTypeEncoder, ComponentTypeSection, CoreTypeEncoder, CoreTypeSection, InstanceType, ModuleArg, ModuleSection, NameMap, NestedComponentSection};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentFuncType, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration, ModuleTypeDeclaration, RecGroup, SubType};
use crate::{Component, Module};
use crate::encode::component::assign_new::ActualIds;
use crate::encode::component::collect::SubItemPlan;
use crate::encode::component::fix_indices_new::FixIndices;
use crate::ir::component::Names;
use crate::ir::component::visitor::{walk_topological, ComponentVisitor, ItemKind, VisitCtx};
use crate::ir::types::CustomSection;

pub(crate) fn encode_internal_new(
    comp: &Component,
    ids: &ActualIds,
) -> wasm_encoder::Component {
    let mut encoder = Encoder::new(ids);
    walk_topological(comp, &mut encoder);
    
    let encoded_comp = encoder.comp_stack.pop().unwrap();
    debug_assert!(encoder.comp_stack.is_empty());

    encoded_comp
}

struct Encoder<'a> {
    ids: &'a ActualIds,
    comp_stack: Vec<wasm_encoder::Component>,
    reencode: RoundtripReencoder
}
impl<'a> Encoder<'a> {
    pub fn new(ids: &'a ActualIds) -> Encoder<'a> {
        Self {
            ids,
            comp_stack: vec![],
            reencode: RoundtripReencoder
        }
    }
    fn handle_enter_comp(&mut self) {
        self.comp_stack.push(wasm_encoder::Component::new());
    }
    fn handle_exit_comp(enc_comp: &mut wasm_encoder::Component, comp: &Component<'_>) {
        // Handle the name section
        let mut name_sec = wasm_encoder::ComponentNameSection::new();

        if let Some(comp_name) = &comp.component_name {
            name_sec.component(comp_name);
        }

        // TODO -- does the order here matter for names in the map?
        //         might need to fix indices here!
        name_sec.core_funcs(&encode_name_section(&comp.core_func_names));
        name_sec.core_tables(&encode_name_section(&comp.table_names));
        name_sec.core_memories(&encode_name_section(&comp.memory_names));
        name_sec.core_tags(&encode_name_section(&comp.tag_names));
        name_sec.core_globals(&encode_name_section(&comp.global_names));
        name_sec.core_types(&encode_name_section(&comp.core_type_names));
        name_sec.core_modules(&encode_name_section(&comp.module_names));
        name_sec.core_instances(&encode_name_section(&comp.core_instances_names));
        name_sec.funcs(&encode_name_section(&comp.func_names));
        name_sec.values(&encode_name_section(&comp.value_names));
        name_sec.types(&encode_name_section(&comp.type_names));
        name_sec.components(&encode_name_section(&comp.components_names));
        name_sec.instances(&encode_name_section(&comp.instance_names));

        // Add the name section back to the component
        enc_comp.section(&name_sec);
    }
}
impl ComponentVisitor<'_> for Encoder<'_> {
    fn enter_root_component(&mut self, _cx: &VisitCtx<'_>, _component: &Component<'_>) {
        self.handle_enter_comp();
    }
    fn exit_root_component(&mut self, _cx: &VisitCtx<'_>, comp: &Component<'_>) {
        Self::handle_exit_comp(self.comp_stack.last_mut().unwrap(), comp);
    }
    fn enter_component(&mut self, _cx: &VisitCtx<'_>, _id: u32, _comp: &Component<'_>) {
        self.handle_enter_comp();
    }
    fn exit_component(&mut self, _: &VisitCtx<'_>, _id: u32, comp: &Component<'_>) {
        let mut nested_comp = self.comp_stack.pop().unwrap();
        Self::handle_exit_comp(&mut nested_comp, comp);

        self.comp_stack.last_mut().unwrap().section(&NestedComponentSection(&nested_comp));
    }
    fn visit_module(&mut self, _cx: &VisitCtx<'_>, _id: u32, module: &Module<'_>) {
        encode_module_section(module, self.comp_stack.last_mut().unwrap());
    }
    fn enter_comp_type(&mut self, _cx: &VisitCtx<'_>, _id: u32, _comp_type: &ComponentType<'_>) {
        todo!()
    }
    fn visit_comp_type_decl(&mut self, _cx: &VisitCtx<'_>, _decl_idx: usize, _id: u32, _parent: &ComponentType<'_>, _decl: &ComponentTypeDeclaration<'_>) {
        todo!()
    }
    fn visit_inst_type_decl(&mut self, _cx: &VisitCtx<'_>, _decl_idx: usize, _id: u32, _parent: &ComponentType<'_>, _decl: &InstanceTypeDeclaration<'_>) {
        todo!()
    }
    fn exit_comp_type(&mut self, _cx: &VisitCtx<'_>, _id: u32, _comp_type: &ComponentType<'_>) {
        todo!()
    }
    fn visit_comp_instance(&mut self, cx: &VisitCtx<'_>, _id: u32, instance: &ComponentInstance<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = instance.fix(&None, ids);
        encode_comp_inst_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_canon(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, _id: u32, canon: &CanonicalFunction) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = canon.fix(&None, ids);
        encode_canon_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_alias(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, _id: u32, alias: &ComponentAlias<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = alias.fix(&None, ids);
        encode_alias_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_comp_import(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, _id: u32, import: &ComponentImport<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = import.fix(&None, ids);
        encode_comp_import_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_comp_export(&mut self, cx: &VisitCtx<'_>, _kind: ItemKind, _id: u32, export: &ComponentExport<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = export.fix(&None, ids);
        encode_comp_export_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn enter_core_type(&mut self, _cx: &VisitCtx<'_>, _id: u32, _core_type: &CoreType<'_>) {
        todo!()
    }
    fn visit_module_type_decl(&mut self, _cx: &VisitCtx<'_>, _decl_idx: usize, _id: u32, _parent: &CoreType<'_>, _decl: &ModuleTypeDeclaration<'_>) {
        todo!()
    }
    fn exit_core_type(&mut self, _cx: &VisitCtx<'_>, _id: u32, _core_type: &CoreType<'_>) {
        todo!()
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'_>, _id: u32, inst: &Instance<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = inst.fix(&None, ids);
        encode_inst_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_start_section(&mut self, cx: &VisitCtx<'_>, start: &ComponentStartFunction) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = start.fix(&None, ids);
        encode_start_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
    fn visit_custom_section(&mut self, cx: &VisitCtx<'_>, sect: &CustomSection<'_>) {
        let ids = self.ids.get_scope(cx.inner.scope_stack.curr_space_id());
        let fixed = sect.fix(&None, ids);
        encode_custom_section(&fixed, self.comp_stack.last_mut().unwrap(), &mut self.reencode);
    }
}

fn encode_name_section(names: &Names) -> NameMap {
    let mut enc_names = NameMap::default();

    for (idx, name) in names.names.iter() {
        enc_names.append(*idx, name)
    }
    enc_names
}


fn encode_module_section(module: &Module, component: &mut wasm_encoder::Component) {
    component.section(&ModuleSection(&module.encode_internal(false).0));
}
fn encode_comp_ty_section(
    comp_ty: &ComponentType,
    plan: &Option<SubItemPlan>,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(comp_ty);
    let mut section = ComponentTypeSection::new();

    match comp_ty {
        ComponentType::Defined(comp_ty) => {
            encode_comp_defined_ty(comp_ty, section.defined_type(), reencode)
        }
        ComponentType::Func(func_ty) => encode_comp_func_ty(func_ty, section.function(), reencode),
        ComponentType::Component(decls) => {
            let mut new_comp = wasm_encoder::ComponentType::new();
            for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                let decl = &decls[*idx];
                encode_comp_ty_decl(decl, subplan, &mut new_comp, component, reencode, ctx);
            }
            section.component(&new_comp);
        }
        ComponentType::Instance(decls) => {
            let mut ity = InstanceType::new();
            for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                let decl = &decls[*idx];
                encode_inst_ty_decl(decl, subplan, &mut ity, component, reencode, ctx);
            }
            section.instance(&ity);
        }
        ComponentType::Resource { rep, dtor } => {
            section.resource(reencode.val_type(*rep).unwrap(), *dtor);
        }
    }

    component.section(&section);
    ctx.inner.maybe_exit_scope(comp_ty);
}
fn encode_comp_inst_section(
    comp_inst: &ComponentInstance,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    let mut instances = wasm_encoder::ComponentInstanceSection::new();

    match comp_inst {
        ComponentInstance::Instantiate {
            component_index,
            args,
        } => {
            instances.instantiate(
                *component_index,
                args.iter().map(|arg| {
                    (
                        arg.name,
                        reencode.component_export_kind(arg.kind),
                        arg.index,
                    )
                }),
            );
        }
        ComponentInstance::FromExports(export) => {
            instances.export_items(export.iter().map(|value| {
                (
                    value.name.0,
                    reencode.component_export_kind(value.kind),
                    value.index,
                )
            }));
        }
    }

    component.section(&instances);
}
fn encode_canon_section(
    canon: &CanonicalFunction,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

    match canon {
        CanonicalFunction::Lift {
            core_func_index,
            type_index,
            options,
        } => {
            canon_sec.lift(
                *core_func_index,
                *type_index,
                options.iter().map(|canon| {
                    do_reencode(
                        *canon,
                        RoundtripReencoder::canonical_option,
                        reencode,
                        "canonical option",
                    )
                }),
            );
        }
        CanonicalFunction::Lower {
            func_index,
            options,
        } => {
            canon_sec.lower(
                *func_index,
                options.iter().map(|canon| {
                    do_reencode(
                        *canon,
                        RoundtripReencoder::canonical_option,
                        reencode,
                        "canonical option",
                    )
                }),
            );
        }
        CanonicalFunction::ResourceNew { resource } => {
            canon_sec.resource_new(*resource);
        }
        CanonicalFunction::ResourceDrop { resource } => {
            canon_sec.resource_drop(*resource);
        }
        CanonicalFunction::ResourceRep { resource } => {
            canon_sec.resource_rep(*resource);
        }
        CanonicalFunction::ResourceDropAsync { resource } => {
            canon_sec.resource_drop_async(*resource);
        }
        CanonicalFunction::ThreadAvailableParallelism => {
            canon_sec.thread_available_parallelism();
        }
        CanonicalFunction::BackpressureDec => {
            canon_sec.backpressure_dec();
        }
        CanonicalFunction::BackpressureInc => {
            canon_sec.backpressure_inc();
        }
        CanonicalFunction::TaskReturn { result, options } => {
            canon_sec.task_return(
                result.map(|v| v.into()),
                options.iter().map(|opt| (*opt).into()),
            );
        }
        CanonicalFunction::WaitableSetNew => {
            canon_sec.waitable_set_new();
        }
        CanonicalFunction::WaitableSetWait {
            cancellable,
            memory,
        } => {
            // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
            canon_sec.waitable_set_wait(*cancellable, *memory);
        }
        CanonicalFunction::WaitableSetPoll {
            cancellable,
            memory,
        } => {
            // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
            canon_sec.waitable_set_poll(*cancellable, *memory);
        }
        CanonicalFunction::WaitableSetDrop => {
            canon_sec.waitable_set_drop();
        }
        CanonicalFunction::WaitableJoin => {
            canon_sec.waitable_join();
        }
        CanonicalFunction::SubtaskDrop => {
            canon_sec.subtask_drop();
        }
        CanonicalFunction::StreamNew { ty } => {
            canon_sec.stream_new(*ty);
        }
        CanonicalFunction::StreamRead { ty, options } => {
            canon_sec.stream_read(*ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::StreamWrite { ty, options } => {
            canon_sec.stream_write(*ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::StreamCancelRead { async_, ty } => {
            canon_sec.stream_cancel_read(*ty, *async_);
        }
        CanonicalFunction::StreamCancelWrite { async_, ty } => {
            canon_sec.stream_cancel_write(*ty, *async_);
        }
        CanonicalFunction::FutureNew { ty } => {
            canon_sec.future_new(*ty);
        }
        CanonicalFunction::FutureRead { ty, options } => {
            canon_sec.future_read(*ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::FutureWrite { ty, options } => {
            canon_sec.future_write(*ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::FutureCancelRead { async_, ty } => {
            canon_sec.future_cancel_read(*ty, *async_);
        }
        CanonicalFunction::FutureCancelWrite { async_, ty } => {
            canon_sec.future_cancel_write(*ty, *async_);
        }
        CanonicalFunction::ErrorContextNew { options } => {
            canon_sec.error_context_new(options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::ErrorContextDebugMessage { options } => {
            canon_sec.error_context_debug_message(options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::ErrorContextDrop => {
            canon_sec.error_context_drop();
        }
        CanonicalFunction::ThreadSpawnRef { func_ty_index } => {
            canon_sec.thread_spawn_ref(*func_ty_index);
        }
        CanonicalFunction::ThreadSpawnIndirect {
            func_ty_index,
            table_index,
        } => {
            canon_sec.thread_spawn_indirect(*func_ty_index, *table_index);
        }
        CanonicalFunction::TaskCancel => {
            canon_sec.task_cancel();
        }
        CanonicalFunction::ContextGet(i) => {
            canon_sec.context_get(*i);
        }
        CanonicalFunction::ContextSet(i) => {
            canon_sec.context_set(*i);
        }
        CanonicalFunction::SubtaskCancel { async_ } => {
            canon_sec.subtask_cancel(*async_);
        }
        CanonicalFunction::StreamDropReadable { ty } => {
            canon_sec.stream_drop_readable(*ty);
        }
        CanonicalFunction::StreamDropWritable { ty } => {
            canon_sec.stream_drop_writable(*ty);
        }
        CanonicalFunction::FutureDropReadable { ty } => {
            canon_sec.future_drop_readable(*ty);
        }
        CanonicalFunction::FutureDropWritable { ty } => {
            canon_sec.future_drop_writable(*ty);
        }
        CanonicalFunction::ThreadYield { cancellable } => {
            canon_sec.thread_yield(*cancellable);
        }
        CanonicalFunction::ThreadIndex => {
            canon_sec.thread_index();
        }
        CanonicalFunction::ThreadNewIndirect {
            func_ty_index,
            table_index,
        } => {
            canon_sec.thread_new_indirect(*func_ty_index, *table_index);
        }
        CanonicalFunction::ThreadSwitchTo { cancellable } => {
            canon_sec.thread_switch_to(*cancellable);
        }
        CanonicalFunction::ThreadSuspend { cancellable } => {
            canon_sec.thread_suspend(*cancellable);
        }
        CanonicalFunction::ThreadResumeLater => {
            canon_sec.thread_resume_later();
        }
        CanonicalFunction::ThreadYieldTo { cancellable } => {
            canon_sec.thread_yield_to(*cancellable);
        }
    }
    component.section(&canon_sec);
}
fn encode_alias_section(
    alias: &ComponentAlias,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    let new_a = into_wasm_encoder_alias(alias, reencode);

    let mut alias_section = ComponentAliasSection::new();
    alias_section.alias(new_a);
    component.section(&alias_section);
}
fn encode_comp_import_section(
    import: &ComponentImport,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    let mut imports = wasm_encoder::ComponentImportSection::new();

    let ty = do_reencode(
        import.ty,
        RoundtripReencoder::component_type_ref,
        reencode,
        "component import",
    );
    imports.import(import.name.0, ty);

    component.section(&imports);
}
fn encode_comp_export_section(
    export: &ComponentExport,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
) {
    let mut exports = wasm_encoder::ComponentExportSection::new();

    let ty = export.ty.map(|ty| {
        do_reencode(
            ty,
            RoundtripReencoder::component_type_ref,
            reencode,
            "component export",
        )
    });

    exports.export(
        export.name.0,
        reencode.component_export_kind(export.kind),
        export.index,
        ty,
    );

    component.section(&exports);
}
fn encode_core_ty_section(
    core_ty: &CoreType,
    plan: &Option<SubItemPlan>,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(core_ty);
    let mut type_section = CoreTypeSection::new();
    match core_ty {
        CoreType::Rec(group) => {
            encode_rec_group_in_core_ty(group, &mut type_section, reencode, ctx)
        }
        CoreType::Module(decls) => {
            encode_module_type_decls(plan, decls, type_section.ty(), reencode, ctx)
        }
    }
    component.section(&type_section);
    ctx.inner.maybe_exit_scope(core_ty);
}
fn encode_inst_section(
    inst: &Instance,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
) {
    let mut instances = wasm_encoder::InstanceSection::new();

    match inst {
        Instance::Instantiate { module_index, args } => {
            instances.instantiate(
                *module_index,
                args.iter()
                    .map(|arg| (arg.name, ModuleArg::Instance(arg.index))),
            );
        }
        Instance::FromExports(exports) => {
            instances.export_items(exports.iter().map(|export| {
                (
                    export.name,
                    wasm_encoder::ExportKind::from(export.kind),
                    export.index,
                )
            }));
        }
    }

    component.section(&instances);
}
fn encode_start_section(
    start: &ComponentStartFunction,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
) {
    component.section(&wasm_encoder::ComponentStartSection {
        function_index: start.func_index,
        args: start.arguments.clone(),
        results: start.results,
    });
}
fn encode_custom_section(
    custom: &CustomSection,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
) {
    component.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(custom.name),
        data: custom.data.clone(),
    });
}

// === The inner structs ===

fn encode_comp_defined_ty(
    ty: &ComponentDefinedType,
    enc: ComponentDefinedTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    match ty {
        ComponentDefinedType::Primitive(p) => {
            enc.primitive(wasm_encoder::PrimitiveValType::from(*p))
        }
        ComponentDefinedType::Record(records) => {
            enc.record(
                records
                    .iter()
                    .map(|(n, ty)| (*n, reencode.component_val_type(*ty))),
            );
        }
        ComponentDefinedType::Variant(variants) => enc.variant(variants.iter().map(|variant| {
            (
                variant.name,
                variant.ty.map(|ty| reencode.component_val_type(ty)),
                variant.refines,
            )
        })),
        ComponentDefinedType::List(l) => enc.list(reencode.component_val_type(*l)),
        ComponentDefinedType::Tuple(tup) => enc.tuple(
            tup.iter()
                .map(|val_type| reencode.component_val_type(*val_type)),
        ),
        ComponentDefinedType::Flags(flags) => enc.flags(flags.clone().into_vec()),
        ComponentDefinedType::Enum(en) => enc.enum_type(en.clone().into_vec()),
        ComponentDefinedType::Option(opt) => enc.option(reencode.component_val_type(*opt)),
        ComponentDefinedType::Result { ok, err } => enc.result(
            ok.map(|val_type| reencode.component_val_type(val_type)),
            err.map(|val_type| reencode.component_val_type(val_type)),
        ),
        ComponentDefinedType::Own(id) => enc.own(*id),
        ComponentDefinedType::Borrow(id) => enc.borrow(*id),
        ComponentDefinedType::Future(opt) => {
            enc.future(opt.map(|opt| reencode.component_val_type(opt)))
        }
        ComponentDefinedType::Stream(opt) => {
            enc.stream(opt.map(|opt| reencode.component_val_type(opt)))
        }
        ComponentDefinedType::FixedSizeList(ty, i) => {
            enc.fixed_size_list(reencode.component_val_type(*ty), *i)
        }
        ComponentDefinedType::Map(key_ty, val_ty) => enc.map(
            reencode.component_val_type(*key_ty),
            reencode.component_val_type(*val_ty),
        ),
    }
}

fn encode_comp_func_ty(
    ty: &ComponentFuncType,
    mut enc: ComponentFuncTypeEncoder,
    reencode: &mut RoundtripReencoder,
) {
    enc.async_(ty.async_);
    enc.params(
        ty.params
            .iter()
            .map(|(name, ty)| (*name, reencode.component_val_type(*ty))),
    );
    enc.result(ty.result.map(|v| reencode.component_val_type(v)));
}

fn encode_comp_ty_decl(
    ty: &ComponentTypeDeclaration,
    subitem_plan: &Option<SubItemPlan>,
    new_comp_ty: &mut wasm_encoder::ComponentType,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(ty);
    match ty {
        ComponentTypeDeclaration::CoreType(core_ty) => {
            encode_core_ty_in_comp_ty(core_ty, subitem_plan, new_comp_ty, reencode, ctx)
        }
        ComponentTypeDeclaration::Type(comp_ty) => encode_comp_ty(
            comp_ty,
            subitem_plan,
            new_comp_ty.ty(),
            component,
            reencode,
            ctx,
        ),
        ComponentTypeDeclaration::Alias(a) => encode_alias_in_comp_ty(a, new_comp_ty, reencode),
        ComponentTypeDeclaration::Export { name, ty } => {
            let ty = do_reencode(
                *ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component type",
            );
            new_comp_ty.export(name.0, ty);
        }
        ComponentTypeDeclaration::Import(imp) => {
            let ty = do_reencode(
                imp.ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component type",
            );
            new_comp_ty.import(imp.name.0, ty);
        }
    }
    ctx.inner.maybe_exit_scope(ty);
}
fn encode_alias_in_comp_ty(
    alias: &ComponentAlias,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
) {
    let new_a = into_wasm_encoder_alias(alias, reencode);
    comp_ty.alias(new_a);
}
fn encode_rec_group_in_core_ty(
    group: &RecGroup,
    enc: &mut CoreTypeSection,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    let types = into_wasm_encoder_recgroup(group, reencode, ctx);

    if group.is_explicit_rec_group() {
        enc.ty().core().rec(types);
    } else {
        // it's implicit!
        for subty in types {
            enc.ty().core().subtype(&subty);
        }
    }
}

fn encode_core_ty_in_comp_ty(
    core_ty: &CoreType,
    subitem_plan: &Option<SubItemPlan>,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(core_ty);
    match core_ty {
        CoreType::Rec(recgroup) => {
            for sub in recgroup.types() {
                encode_subtype(sub, comp_ty.core_type().core(), reencode);
            }
        }
        CoreType::Module(decls) => {
            encode_module_type_decls(subitem_plan, decls, comp_ty.core_type(), reencode, ctx)
        }
    }
    ctx.inner.maybe_exit_scope(core_ty);
}

fn encode_inst_ty_decl(
    inst: &InstanceTypeDeclaration,
    subitem_plan: &Option<SubItemPlan>,
    ity: &mut InstanceType,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(inst);
    match inst {
        InstanceTypeDeclaration::CoreType(core_ty) => {
            encode_core_ty_in_inst_ty(core_ty, subitem_plan, ity, reencode, ctx)
        }
        InstanceTypeDeclaration::Type(ty) => {
            let enc = ity.ty();
            encode_comp_ty(ty, subitem_plan, enc, component, reencode, ctx);
        }
        InstanceTypeDeclaration::Alias(alias) => match alias {
            ComponentAlias::InstanceExport {
                kind,
                instance_index,
                name,
            } => {
                ity.alias(Alias::InstanceExport {
                    instance: *instance_index,
                    kind: reencode.component_export_kind(*kind),
                    name,
                });
            }
            ComponentAlias::CoreInstanceExport {
                kind,
                instance_index,
                name,
            } => {
                ity.alias(Alias::CoreInstanceExport {
                    instance: *instance_index,
                    kind: do_reencode(
                        *kind,
                        RoundtripReencoder::export_kind,
                        reencode,
                        "export kind",
                    ),
                    name,
                });
            }
            ComponentAlias::Outer { kind, count, index } => {
                ity.alias(Alias::Outer {
                    kind: reencode.component_outer_alias_kind(*kind),
                    count: *count,
                    index: *index,
                });
            }
        },
        InstanceTypeDeclaration::Export { name, ty } => {
            ity.export(
                name.0,
                do_reencode(
                    *ty,
                    RoundtripReencoder::component_type_ref,
                    reencode,
                    "component type",
                ),
            );
        }
    }
    ctx.inner.maybe_exit_scope(inst);
}
fn encode_core_ty_in_inst_ty(
    core_ty: &CoreType,
    subitem_plan: &Option<SubItemPlan>,
    inst_ty: &mut InstanceType,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(core_ty);
    match core_ty {
        CoreType::Rec(recgroup) => {
            for sub in recgroup.types() {
                encode_subtype(sub, inst_ty.core_type().core(), reencode);
            }
        }
        CoreType::Module(decls) => {
            encode_module_type_decls(subitem_plan, decls, inst_ty.core_type(), reencode, ctx)
        }
    }
    ctx.inner.maybe_exit_scope(core_ty);
}

fn encode_comp_ty(
    ty: &ComponentType,
    subitem_plan: &Option<SubItemPlan>,
    enc: ComponentTypeEncoder,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    ctx.inner.maybe_enter_scope(ty);
    match ty {
        ComponentType::Defined(comp_ty) => {
            encode_comp_defined_ty(comp_ty, enc.defined_type(), reencode)
        }
        ComponentType::Func(func_ty) => encode_comp_func_ty(func_ty, enc.function(), reencode),
        ComponentType::Component(decls) => {
            let mut new_comp = wasm_encoder::ComponentType::new();
            for (idx, subplan) in subitem_plan.as_ref().unwrap().order().iter() {
                encode_comp_ty_decl(
                    &decls[*idx],
                    subplan,
                    &mut new_comp,
                    component,
                    reencode,
                    ctx,
                );
            }
            enc.component(&new_comp);
        }
        ComponentType::Instance(decls) => {
            let mut ity = InstanceType::new();
            if let Some(subplan) = subitem_plan {
                for (idx, subplan) in subplan.order().iter() {
                    encode_inst_ty_decl(&decls[*idx], subplan, &mut ity, component, reencode, ctx);
                }
            }

            enc.instance(&ity);
        }
        ComponentType::Resource { rep, dtor } => {
            enc.resource(reencode.val_type(*rep).unwrap(), *dtor);
        }
    }
    ctx.inner.maybe_exit_scope(ty);
}

fn into_wasm_encoder_alias<'a>(
    alias: &ComponentAlias<'a>,
    reencode: &mut RoundtripReencoder,
) -> Alias<'a> {
    match alias {
        ComponentAlias::InstanceExport {
            kind,
            instance_index,
            name,
        } => Alias::InstanceExport {
            instance: *instance_index,
            kind: reencode.component_export_kind(*kind),
            name,
        },
        ComponentAlias::CoreInstanceExport {
            kind,
            instance_index,
            name,
        } => Alias::CoreInstanceExport {
            instance: *instance_index,
            kind: do_reencode(
                *kind,
                RoundtripReencoder::export_kind,
                reencode,
                "export kind",
            ),
            name,
        },
        ComponentAlias::Outer { kind, count, index } => Alias::Outer {
            kind: reencode.component_outer_alias_kind(*kind),
            count: *count,
            index: *index,
        },
    }
}

pub fn into_wasm_encoder_recgroup(
    group: &RecGroup,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) -> Vec<wasm_encoder::SubType> {
    // ctx.inner.maybe_enter_scope(group);
    //
    // let subtypes = group
    //     .types()
    //     .map(|subty| {
    //         let fixed_subty = subty.fix(&None, ctx);
    //         reencode
    //             .sub_type(fixed_subty)
    //             .unwrap_or_else(|e| panic!("Could not encode type as subtype: {:?}\n\t{e}", subty))
    //     })
    //     .collect::<Vec<_>>();
    //
    // ctx.inner.maybe_exit_scope(group);
    // subtypes
    todo!()
}

pub fn encode_module_type_decls(
    subitem_plan: &Option<SubItemPlan>,
    decls: &[wasmparser::ModuleTypeDeclaration],
    enc: ComponentCoreTypeEncoder,
    reencode: &mut RoundtripReencoder,
    ctx: &mut VisitCtx,
) {
    let mut mty = wasm_encoder::ModuleType::new();
    for (idx, subplan) in subitem_plan.as_ref().unwrap().order().iter() {
        assert!(subplan.is_none());

        let decl = &decls[*idx];
        ctx.inner.maybe_enter_scope(decl);
        match decl {
            wasmparser::ModuleTypeDeclaration::Type(recgroup) => {
                let types = into_wasm_encoder_recgroup(recgroup, reencode, ctx);

                if recgroup.is_explicit_rec_group() {
                    mty.ty().rec(types);
                } else {
                    // it's implicit!
                    for subty in types {
                        mty.ty().subtype(&subty);
                    }
                }
            }
            wasmparser::ModuleTypeDeclaration::Export { name, ty } => {
                mty.export(name, reencode.entity_type(*ty).unwrap());
            }
            wasmparser::ModuleTypeDeclaration::OuterAlias {
                kind: _kind,
                count,
                index,
            } => {
                mty.alias_outer_core_type(*count, *index);
            }
            wasmparser::ModuleTypeDeclaration::Import(import) => {
                mty.import(
                    import.module,
                    import.name,
                    reencode.entity_type(import.ty).unwrap(),
                );
            }
        }
        ctx.inner.maybe_exit_scope(decl);
    }
    enc.module(&mty);
}

fn encode_subtype(subtype: &SubType, enc: CoreTypeEncoder, reencode: &mut RoundtripReencoder) {
    let subty = reencode
        .sub_type(subtype.to_owned())
        .unwrap_or_else(|_| panic!("Could not encode type as subtype: {:?}", subtype));

    enc.subtype(&subty);
}

pub(crate) fn do_reencode<I, O>(
    i: I,
    reencode: fn(&mut RoundtripReencoder, I) -> Result<O, wasm_encoder::reencode::Error>,
    inst: &mut RoundtripReencoder,
    msg: &str,
) -> O {
    match reencode(inst, i) {
        Ok(o) => o,
        Err(e) => panic!("Couldn't encode {} due to error: {}", msg, e),
    }
}

