use wasm_encoder::{Alias, ComponentAliasSection, ComponentDefinedTypeEncoder, ComponentFuncTypeEncoder, ComponentTypeSection, CoreTypeEncoder, CoreTypeSection, ModuleArg, ModuleSection, NameMap, NestedComponentSection};
use wasm_encoder::reencode::{Reencode, ReencodeComponent, RoundtripReencoder};
use wasmparser::{CanonicalFunction, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentFuncType, ComponentImport, ComponentInstance, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, CoreType, Instance, InstanceTypeDeclaration, ModuleTypeDeclaration, RecGroup, SubType};
use crate::{Component, Module};
use crate::encode::component::assign::ActualIds;
use crate::encode::component::fix_indices::FixIndices;
use crate::ir::component::Names;
use crate::ir::component::visitor::{walk_topological, ComponentVisitor, ItemKind, VisitCtx};
use crate::ir::component::visitor::utils::ScopeStack;
use crate::ir::types::CustomSection;

pub(crate) fn encode_internal_new(
    comp: &Component,
    ids: &ActualIds,
) -> wasm_encoder::Component {
    let mut encoder = Encoder::new(ids);
    walk_topological(comp, &mut encoder);
    
    let encoded_comp = encoder.comp_stack.pop().unwrap().component;
    debug_assert!(encoder.comp_stack.is_empty());

    encoded_comp
}

struct Encoder<'a> {
    reencode: RoundtripReencoder,

    ids: &'a ActualIds,
    comp_stack: Vec<CompFrame>,

    // recursive def items!
    type_stack: Vec<TypeFrame>
}
impl<'a> Encoder<'a> {
    pub fn new(ids: &'a ActualIds) -> Encoder<'a> {
        Self {
            reencode: RoundtripReencoder,
            ids,
            comp_stack: vec![],

            type_stack: vec![]
        }
    }
    fn curr_comp_mut(&mut self) -> &mut wasm_encoder::Component {
        &mut self.comp_stack.last_mut().unwrap().component
    }
    fn handle_enter_comp(&mut self) {
        self.comp_stack.push(CompFrame::new());
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
        Self::handle_exit_comp(self.curr_comp_mut(), comp);
    }
    fn enter_component(&mut self, _cx: &VisitCtx<'_>, _id: u32, _comp: &Component<'_>) {
        self.handle_enter_comp();
    }
    fn exit_component(&mut self, _: &VisitCtx<'_>, _id: u32, comp: &Component<'_>) {
        let nested_comp = &mut self.comp_stack.pop().unwrap().component;
        Self::handle_exit_comp(nested_comp, comp);

        self.curr_comp_mut().section(&NestedComponentSection(&nested_comp));
    }
    fn visit_module(&mut self, _: &VisitCtx<'_>, _id: u32, module: &Module<'_>) {
        encode_module_section(module, self.curr_comp_mut());
    }
    fn enter_comp_type(&mut self, cx: &VisitCtx<'_>, _id: u32, ty: &ComponentType<'_>) {
        // always make sure the component type section exists!
        let section = curr_comp_ty_sect_mut(&mut self.comp_stack);
        match self.type_stack.last_mut() {
            Some(TypeFrame::InstTy { ty: ity }) => {
                let new_frame = encode_comp_ty_in_inst_ty(ty, ity, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            }
            Some(TypeFrame::CompTy { ty: cty }) => {
                let new_frame = encode_comp_ty_in_comp_ty(ty, cty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            },
            Some(TypeFrame::ModTy { .. }) => unreachable!(),
            Some(TypeFrame::Nop) | None => {}
        }

        match ty {
            ComponentType::Defined(comp_ty) => {
                encode_comp_defined_ty(comp_ty, section.defined_type(), &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(TypeFrame::Nop);
            }
            ComponentType::Func(func_ty) => {
                encode_comp_func_ty(func_ty, section.function(), &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(TypeFrame::Nop);
            },
            ComponentType::Resource { rep, dtor } => {
                section.resource(self.reencode.val_type(*rep).unwrap(), *dtor);
                self.type_stack.push(TypeFrame::Nop);
            }
            ComponentType::Component(_) => {
                self.type_stack.push(TypeFrame::CompTy {
                    ty: wasm_encoder::ComponentType::new()
                });
            }
            ComponentType::Instance(_) => {
                self.type_stack.push(TypeFrame::InstTy {
                    ty: wasm_encoder::InstanceType::new()
                });
            }
        }
    }
    fn visit_comp_type_decl(&mut self, cx: &VisitCtx<'_>, _: usize, _: u32, _: &ComponentType<'_>, decl: &ComponentTypeDeclaration<'_>) {
        match self.type_stack.last_mut().unwrap() {
            TypeFrame::CompTy { ty } => {
                encode_comp_ty_decl(decl, ty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
            },
            TypeFrame::InstTy { .. }
            | TypeFrame::ModTy { .. }
            | TypeFrame::Nop=> unreachable!(),
        }
    }
    fn visit_inst_type_decl(&mut self, cx: &VisitCtx<'_>, _: usize, _: u32, _: &ComponentType<'_>, decl: &InstanceTypeDeclaration<'_>) {
        match self.type_stack.last_mut().unwrap() {
            TypeFrame::InstTy { ty } => {
                encode_inst_ty_decl(decl, ty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
            },
            TypeFrame::CompTy { .. }
            | TypeFrame::ModTy { .. }
            | TypeFrame::Nop => unreachable!(),
        }
    }
    fn exit_comp_type(&mut self, _: &VisitCtx<'_>, _: u32, _: &ComponentType<'_>) {
        let CompFrame {comp_type_section, component, .. } = curr_comp_frame(&mut self.comp_stack);
        let section = comp_type_section.as_mut().unwrap();
        match self.type_stack.pop() {
            Some(TypeFrame::CompTy { ty }) => {
                if let Some(TypeFrame::CompTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.ty().component(&ty);
                } else if let Some(TypeFrame::InstTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.ty().component(&ty);
                } else {
                    section.component(&ty);
                }
            }
            Some(TypeFrame::InstTy { ty }) => {
                if let Some(TypeFrame::InstTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.ty().instance(&ty); // attach to parent instance
                } else if let Some(TypeFrame::CompTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.ty().instance(&ty); // attach to enclosing ComponentType
                } else {
                    // top-level type, attach to comp_type_section
                    section.instance(&ty);
                }
            }
            Some(TypeFrame::ModTy { .. }) => unreachable!(),
            Some(TypeFrame::Nop)
            | None => {}
        }

        if self.type_stack.is_empty() {
            component.section(section);
            *comp_type_section = None;
        }
    }
    fn visit_comp_instance(&mut self, cx: &VisitCtx<'_>, _: u32, instance: &ComponentInstance<'_>) {
        encode_comp_inst_section(instance, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_canon(&mut self, cx: &VisitCtx<'_>, _: ItemKind, _: u32, canon: &CanonicalFunction) {
        encode_canon_section(canon, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_alias(&mut self, cx: &VisitCtx<'_>, _: ItemKind, _: u32, alias: &ComponentAlias<'_>) {
        encode_alias_section(alias, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_comp_import(&mut self, cx: &VisitCtx<'_>, _: ItemKind, _: u32, import: &ComponentImport<'_>) {
        encode_comp_import_section(import, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_comp_export(&mut self, cx: &VisitCtx<'_>, _: ItemKind, _: u32, export: &ComponentExport<'_>) {
        encode_comp_export_section(export, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn enter_core_rec_group(&mut self, cx: &VisitCtx<'_>, _: usize, ty: &CoreType<'_>) {
        // always make sure the core type section exists!
        let section = curr_core_ty_sect_mut(&mut self.comp_stack);
        match self.type_stack.last_mut() {
            Some(TypeFrame::InstTy { ty: ity }) => {
                let new_frame = encode_core_ty_from_inst_ty(ty, ity, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            },
            Some(TypeFrame::CompTy { ty: cty }) => {
                let new_frame = encode_core_ty_from_comp_ty(ty, cty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            },
            Some(TypeFrame::ModTy { .. }) => unreachable!(),
            Some(TypeFrame::Nop)
            | None => {}
        }

        match ty {
            CoreType::Rec(group) => {
                encode_rec_group_in_core_ty(group, section, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
            }
            _ => unreachable!()
        }
    }
    fn exit_core_rec_group(&mut self, _: &VisitCtx<'_>) {
        let CompFrame {core_type_section, component, .. } = curr_comp_frame(&mut self.comp_stack);
        let section = core_type_section.as_mut().unwrap();

        component.section(section);
        *core_type_section = None;
    }
    fn enter_core_type(&mut self, cx: &VisitCtx<'_>, _id: u32, ty: &CoreType<'_>) {
        // always make sure the core type section exists!
        curr_core_ty_sect_mut(&mut self.comp_stack);
        match self.type_stack.last_mut() {
            Some(TypeFrame::InstTy { ty: ity }) => {
                let new_frame = encode_core_ty_from_inst_ty(ty, ity, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            },
            Some(TypeFrame::CompTy { ty: cty }) => {
                let new_frame = encode_core_ty_from_comp_ty(ty, cty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
                self.type_stack.push(new_frame);
                return;
            },
            Some(TypeFrame::ModTy { .. }) => unreachable!(),
            Some(TypeFrame::Nop)
            | None => {}
        }

        match ty {
            CoreType::Rec(_) => unreachable!(),
            CoreType::Module(_) => {
                self.type_stack.push(TypeFrame::ModTy {
                    ty: wasm_encoder::ModuleType::new()
                });
            }
        }
    }
    fn visit_module_type_decl(&mut self, cx: &VisitCtx<'_>, _decl_idx: usize, _id: u32, _parent: &CoreType<'_>, decl: &ModuleTypeDeclaration<'_>) {
        match self.type_stack.last_mut().unwrap() {
            TypeFrame::ModTy { ty } => {
                encode_module_type_decl(decl, ty, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
            },
            TypeFrame::CompTy { .. }
            | TypeFrame::InstTy { .. }
            | TypeFrame::Nop => unreachable!(),
        }
    }
    fn exit_core_type(&mut self, _cx: &VisitCtx<'_>, _id: u32, _core_type: &CoreType<'_>) {
        let CompFrame {core_type_section, component, .. } = curr_comp_frame(&mut self.comp_stack);
        let section = core_type_section.as_mut().unwrap();
        match self.type_stack.pop() {
            Some(TypeFrame::ModTy { ty }) => {
                if let Some(TypeFrame::CompTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.core_type().module(&ty);
                } else if let Some(TypeFrame::InstTy { ty: parent }) = self.type_stack.last_mut() {
                    parent.core_type().module(&ty);
                } else {
                    section.ty().module(&ty);
                }
            },
            Some(TypeFrame::CompTy { .. })
            | Some(TypeFrame::InstTy { .. }) => unreachable!(),
            Some(TypeFrame::Nop)
            | None => {}
        }

        if self.type_stack.is_empty() {
            component.section(section);
            *core_type_section = None;
        }
    }
    fn visit_core_instance(&mut self, cx: &VisitCtx<'_>, _: u32, inst: &Instance<'_>) {
        encode_inst_section(inst, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_start_section(&mut self, cx: &VisitCtx<'_>, start: &ComponentStartFunction) {
        encode_start_section(start, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
    fn visit_custom_section(&mut self, cx: &VisitCtx<'_>, sect: &CustomSection<'_>) {
        encode_custom_section(sect, &mut self.comp_stack.last_mut().unwrap().component, &mut self.reencode, &self.ids, &cx.inner.scope_stack);
    }
}
struct CompFrame {
    component: wasm_encoder::Component,
    comp_type_section: Option<ComponentTypeSection>,
    core_type_section: Option<CoreTypeSection>,
}
impl CompFrame {
    fn new() -> Self {
        Self {
            component: wasm_encoder::Component::new(),
            comp_type_section: None,
            core_type_section: None,
        }
    }
}

enum TypeFrame {
    CompTy { ty: wasm_encoder::ComponentType },
    InstTy { ty: wasm_encoder::InstanceType },
    ModTy { ty: wasm_encoder::ModuleType },
    Nop
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
fn encode_comp_inst_section(
    instance: &ComponentInstance,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let comp_inst = instance.fix(ids, scope_stack);
    let mut instances = wasm_encoder::ComponentInstanceSection::new();

    match comp_inst {
        ComponentInstance::Instantiate {
            component_index,
            args,
        } => {
            instances.instantiate(
                component_index,
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
    c: &CanonicalFunction,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let canon = c.fix(ids, scope_stack);
    let mut canon_sec = wasm_encoder::CanonicalFunctionSection::new();

    match canon {
        CanonicalFunction::Lift {
            core_func_index,
            type_index,
            options,
        } => {
            canon_sec.lift(
                core_func_index,
                type_index,
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
                func_index,
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
            canon_sec.resource_new(resource);
        }
        CanonicalFunction::ResourceDrop { resource } => {
            canon_sec.resource_drop(resource);
        }
        CanonicalFunction::ResourceRep { resource } => {
            canon_sec.resource_rep(resource);
        }
        CanonicalFunction::ResourceDropAsync { resource } => {
            canon_sec.resource_drop_async(resource);
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
            canon_sec.waitable_set_wait(cancellable, memory);
        }
        CanonicalFunction::WaitableSetPoll {
            cancellable,
            memory,
        } => {
            // NOTE: There's a discrepancy in naming here. `cancellable` refers to the same bit as `async_`
            canon_sec.waitable_set_poll(cancellable, memory);
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
            canon_sec.stream_new(ty);
        }
        CanonicalFunction::StreamRead { ty, options } => {
            canon_sec.stream_read(ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::StreamWrite { ty, options } => {
            canon_sec.stream_write(ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::StreamCancelRead { async_, ty } => {
            canon_sec.stream_cancel_read(ty, async_);
        }
        CanonicalFunction::StreamCancelWrite { async_, ty } => {
            canon_sec.stream_cancel_write(ty, async_);
        }
        CanonicalFunction::FutureNew { ty } => {
            canon_sec.future_new(ty);
        }
        CanonicalFunction::FutureRead { ty, options } => {
            canon_sec.future_read(ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::FutureWrite { ty, options } => {
            canon_sec.future_write(ty, options.iter().map(|opt| (*opt).into()));
        }
        CanonicalFunction::FutureCancelRead { async_, ty } => {
            canon_sec.future_cancel_read(ty, async_);
        }
        CanonicalFunction::FutureCancelWrite { async_, ty } => {
            canon_sec.future_cancel_write(ty, async_);
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
            canon_sec.thread_spawn_ref(func_ty_index);
        }
        CanonicalFunction::ThreadSpawnIndirect {
            func_ty_index,
            table_index,
        } => {
            canon_sec.thread_spawn_indirect(func_ty_index, table_index);
        }
        CanonicalFunction::TaskCancel => {
            canon_sec.task_cancel();
        }
        CanonicalFunction::ContextGet(i) => {
            canon_sec.context_get(i);
        }
        CanonicalFunction::ContextSet(i) => {
            canon_sec.context_set(i);
        }
        CanonicalFunction::SubtaskCancel { async_ } => {
            canon_sec.subtask_cancel(async_);
        }
        CanonicalFunction::StreamDropReadable { ty } => {
            canon_sec.stream_drop_readable(ty);
        }
        CanonicalFunction::StreamDropWritable { ty } => {
            canon_sec.stream_drop_writable(ty);
        }
        CanonicalFunction::FutureDropReadable { ty } => {
            canon_sec.future_drop_readable(ty);
        }
        CanonicalFunction::FutureDropWritable { ty } => {
            canon_sec.future_drop_writable(ty);
        }
        CanonicalFunction::ThreadYield { cancellable } => {
            canon_sec.thread_yield(cancellable);
        }
        CanonicalFunction::ThreadIndex => {
            canon_sec.thread_index();
        }
        CanonicalFunction::ThreadNewIndirect {
            func_ty_index,
            table_index,
        } => {
            canon_sec.thread_new_indirect(func_ty_index, table_index);
        }
        CanonicalFunction::ThreadSwitchTo { cancellable } => {
            canon_sec.thread_switch_to(cancellable);
        }
        CanonicalFunction::ThreadSuspend { cancellable } => {
            canon_sec.thread_suspend(cancellable);
        }
        CanonicalFunction::ThreadResumeLater => {
            canon_sec.thread_resume_later();
        }
        CanonicalFunction::ThreadYieldTo { cancellable } => {
            canon_sec.thread_yield_to(cancellable);
        }
    }
    component.section(&canon_sec);
}
fn encode_alias_section(
    a: &ComponentAlias,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let alias = a.fix(ids, scope_stack);
    let new_a = into_wasm_encoder_alias(&alias, reencode);

    let mut alias_section = ComponentAliasSection::new();
    alias_section.alias(new_a);
    component.section(&alias_section);
}
fn encode_comp_import_section(
    i: &ComponentImport,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let import = i.fix(ids, scope_stack);
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
    e: &ComponentExport,
    component: &mut wasm_encoder::Component,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let export = e.fix(ids, scope_stack);
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
fn encode_inst_section(
    i: &Instance,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let inst = i.fix(ids, scope_stack);
    let mut instances = wasm_encoder::InstanceSection::new();

    match inst {
        Instance::Instantiate { module_index, args } => {
            instances.instantiate(
                module_index,
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
    s: &ComponentStartFunction,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let start = s.fix(ids, scope_stack);
    component.section(&wasm_encoder::ComponentStartSection {
        function_index: start.func_index,
        args: start.arguments.clone(),
        results: start.results,
    });
}
fn encode_custom_section(
    s: &CustomSection,
    component: &mut wasm_encoder::Component,
    _: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let custom = s.fix(ids, scope_stack);
    component.section(&wasm_encoder::CustomSection {
        name: std::borrow::Cow::Borrowed(custom.name),
        data: custom.data.clone(),
    });
}

// === The inner structs ===

fn encode_comp_defined_ty(
    t: &ComponentDefinedType,
    enc: ComponentDefinedTypeEncoder,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let ty = t.fix(ids, scope_stack);
    match ty {
        ComponentDefinedType::Primitive(p) => {
            enc.primitive(wasm_encoder::PrimitiveValType::from(p))
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
        ComponentDefinedType::List(l) => enc.list(reencode.component_val_type(l)),
        ComponentDefinedType::Tuple(tup) => enc.tuple(
            tup.iter()
                .map(|val_type| reencode.component_val_type(*val_type)),
        ),
        ComponentDefinedType::Flags(flags) => enc.flags(flags.clone().into_vec()),
        ComponentDefinedType::Enum(en) => enc.enum_type(en.clone().into_vec()),
        ComponentDefinedType::Option(opt) => enc.option(reencode.component_val_type(opt)),
        ComponentDefinedType::Result { ok, err } => enc.result(
            ok.map(|val_type| reencode.component_val_type(val_type)),
            err.map(|val_type| reencode.component_val_type(val_type)),
        ),
        ComponentDefinedType::Own(id) => enc.own(id),
        ComponentDefinedType::Borrow(id) => enc.borrow(id),
        ComponentDefinedType::Future(opt) => {
            enc.future(opt.map(|opt| reencode.component_val_type(opt)))
        }
        ComponentDefinedType::Stream(opt) => {
            enc.stream(opt.map(|opt| reencode.component_val_type(opt)))
        }
        ComponentDefinedType::FixedSizeList(ty, i) => {
            enc.fixed_size_list(reencode.component_val_type(ty), i)
        }
        ComponentDefinedType::Map(key_ty, val_ty) => enc.map(
            reencode.component_val_type(key_ty),
            reencode.component_val_type(val_ty),
        ),
    }
}

fn encode_comp_func_ty(
    t: &ComponentFuncType,
    mut enc: ComponentFuncTypeEncoder,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let ty = t.fix(ids, scope_stack);
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
    new_comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    match ty {
        ComponentTypeDeclaration::Alias(a) => encode_alias_in_comp_ty(a, new_comp_ty, reencode, ids, scope_stack),
        ComponentTypeDeclaration::Export { name, ty: t } => {
            let ty = t.fix(ids, scope_stack);
            let ty = do_reencode(
                ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component type",
            );
            new_comp_ty.export(name.0, ty);
        }
        ComponentTypeDeclaration::Import(i) => {
            let imp = i.fix(ids, scope_stack);
            let ty = do_reencode(
                imp.ty,
                RoundtripReencoder::component_type_ref,
                reencode,
                "component type",
            );
            new_comp_ty.import(imp.name.0, ty);
        }
        ComponentTypeDeclaration::CoreType(_)
        | ComponentTypeDeclaration::Type(_) => {}, // handled explicitly in visitor
    }
}
fn encode_alias_in_comp_ty(
    a: &ComponentAlias,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    let alias = a.fix(ids, scope_stack);
    let new_a = into_wasm_encoder_alias(&alias, reencode);
    comp_ty.alias(new_a);
}
fn encode_rec_group_in_core_ty(
    group: &RecGroup,
    enc: &mut CoreTypeSection,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack,
) {
    let types = into_wasm_encoder_recgroup(group, reencode, ids, scope_stack);

    if group.is_explicit_rec_group() {
        enc.ty().core().rec(types);
    } else {
        // it's implicit!
        for subty in types {
            enc.ty().core().subtype(&subty);
        }
    }
}

fn encode_inst_ty_decl(
    inst: &InstanceTypeDeclaration,
    ity: &mut wasm_encoder::InstanceType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    match inst {
        InstanceTypeDeclaration::Alias(a) => {
            let alias = a.fix(ids, scope_stack);
            match alias {
                ComponentAlias::InstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::InstanceExport {
                        instance: instance_index,
                        kind: reencode.component_export_kind(kind),
                        name,
                    });
                }
                ComponentAlias::CoreInstanceExport {
                    kind,
                    instance_index,
                    name,
                } => {
                    ity.alias(Alias::CoreInstanceExport {
                        instance: instance_index,
                        kind: do_reencode(
                            kind,
                            RoundtripReencoder::export_kind,
                            reencode,
                            "export kind",
                        ),
                        name,
                    });
                }
                ComponentAlias::Outer { kind, count, index } => {
                    ity.alias(Alias::Outer {
                        kind: reencode.component_outer_alias_kind(kind),
                        count,
                        index,
                    });
                }
            }
        },
        InstanceTypeDeclaration::Export { name, ty: t } => {
            let ty = t.fix(ids, scope_stack);
            ity.export(
                name.0,
                do_reencode(
                    ty,
                    RoundtripReencoder::component_type_ref,
                    reencode,
                    "component type",
                ),
            );
        }
        InstanceTypeDeclaration::CoreType(_)
        | InstanceTypeDeclaration::Type(_) => {}, // handled explicitly in visitor
    }
}
fn encode_core_ty_from_inst_ty(
    core_ty: &CoreType,
    inst_ty: &mut wasm_encoder::InstanceType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) -> TypeFrame {
    match core_ty {
        CoreType::Rec(r) => {
            let recgroup = r.fix(ids, scope_stack);
            for sub in recgroup.types() {
                encode_subtype(sub, inst_ty.core_type().core(), reencode);
            }
            TypeFrame::Nop
        }
        CoreType::Module(_) => {
            TypeFrame::ModTy { ty: wasm_encoder::ModuleType::new() }
        }
    }
}
fn encode_core_ty_from_comp_ty(
    core_ty: &CoreType,
    comp_ty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) -> TypeFrame {
    match core_ty {
        CoreType::Rec(r) => {
            let recgroup = r.fix(ids, scope_stack);
            for sub in recgroup.types() {
                encode_subtype(sub, comp_ty.core_type().core(), reencode);
            }
            TypeFrame::Nop
        }
        CoreType::Module(_) => {
            TypeFrame::ModTy { ty: wasm_encoder::ModuleType::new() }
        }
    }
}
fn encode_module_type_decl(
    d: &ModuleTypeDeclaration,
    mty: &mut wasm_encoder::ModuleType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) {
    if let ModuleTypeDeclaration::Type(recgroup) = d {
        // special handler for recgroups!
        let types = into_wasm_encoder_recgroup(recgroup, reencode, ids, scope_stack);

        if recgroup.is_explicit_rec_group() {
            mty.ty().rec(types);
        } else {
            // it's implicit!
            for subty in types {
                mty.ty().subtype(&subty);
            }
        }
        return;
    }

    let decl = d.fix(ids, scope_stack);
    match decl {
        ModuleTypeDeclaration::Type(_) => unreachable!(),
        ModuleTypeDeclaration::Export { name, ty } => {
            mty.export(name, reencode.entity_type(ty).unwrap());
        }
        ModuleTypeDeclaration::OuterAlias {
            kind: _kind,
            count,
            index,
        } => {
            mty.alias_outer_core_type(count, index);
        }
        ModuleTypeDeclaration::Import(import) => {
            mty.import(
                import.module,
                import.name,
                reencode.entity_type(import.ty).unwrap(),
            );
        }
    }
}

fn encode_comp_ty_in_inst_ty(
    t: &ComponentType,
    ity: &mut wasm_encoder::InstanceType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) -> TypeFrame {
    // special case for components and instances
    if let ComponentType::Component(_) = t {
        return TypeFrame::CompTy { ty: wasm_encoder::ComponentType::new() };
    } else if let ComponentType::Instance(_) = t {
        return TypeFrame::InstTy { ty: wasm_encoder::InstanceType::new() }
    }

    let ty = t.fix(ids, scope_stack);
    match ty {
        ComponentType::Defined(comp_ty) => {
            encode_comp_defined_ty(&comp_ty, ity.ty().defined_type(), reencode, ids, scope_stack);
            TypeFrame::Nop
        }
        ComponentType::Func(func_ty) => {
            encode_comp_func_ty(&func_ty, ity.ty().function(), reencode, ids, scope_stack);
            TypeFrame::Nop
        },
        ComponentType::Resource { rep, dtor } => {
            ity.ty().resource(reencode.val_type(rep).unwrap(), dtor);
            TypeFrame::Nop
        }
        ComponentType::Component(_)
        | ComponentType::Instance(_) => unreachable!()
    }
}

fn encode_comp_ty_in_comp_ty(
    t: &ComponentType,
    cty: &mut wasm_encoder::ComponentType,
    reencode: &mut RoundtripReencoder,
    ids: &ActualIds,
    scope_stack: &ScopeStack
) -> TypeFrame {
    // special case for components and instances
    if let ComponentType::Component(_) = t {
        return TypeFrame::CompTy { ty: wasm_encoder::ComponentType::new() };
    } else if let ComponentType::Instance(_) = t {
        return TypeFrame::InstTy { ty: wasm_encoder::InstanceType::new() }
    }

    let ty = t.fix(ids, scope_stack);
    match ty {
        ComponentType::Defined(comp_ty) => {
            encode_comp_defined_ty(&comp_ty, cty.ty().defined_type(), reencode, ids, scope_stack);
            TypeFrame::Nop
        }
        ComponentType::Func(func_ty) => {
            encode_comp_func_ty(&func_ty, cty.ty().function(), reencode, ids, scope_stack);
            TypeFrame::Nop
        },
        ComponentType::Resource { rep, dtor } => {
            cty.ty().resource(reencode.val_type(rep).unwrap(), dtor);
            TypeFrame::Nop
        }
        ComponentType::Component(_)
        | ComponentType::Instance(_) => unreachable!()
    }
}

/// NOTE: The alias passed here should already be FIXED
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
    ids: &ActualIds,
    scope_stack: &ScopeStack,
) -> Vec<wasm_encoder::SubType> {
    let subtypes = group
        .types()
        .map(|subty| {
            let fixed_subty = subty.fix(ids, scope_stack);
            reencode
                .sub_type(fixed_subty)
                .unwrap_or_else(|e| panic!("Could not encode type as subtype: {:?}\n\t{e}", subty))
        })
        .collect::<Vec<_>>();

    subtypes
}

/// NOTE: The subtype passed here should already be FIXED
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

fn curr_comp_frame<'b>(comp_stack: &'b mut Vec<CompFrame>) -> &'b mut CompFrame {
    comp_stack.last_mut().unwrap()
}
fn curr_comp_ty_sect_mut<'b>(comp_stack: &'b mut Vec<CompFrame>) -> &'b mut ComponentTypeSection {
    let frame = curr_comp_frame(comp_stack);

    if frame.comp_type_section.is_none() {
        frame.comp_type_section = Some(
            ComponentTypeSection::new()
        );
    }

    frame.comp_type_section.as_mut().unwrap()
}
fn curr_core_ty_sect_mut<'b>(comp_stack: &'b mut Vec<CompFrame>) -> &'b mut CoreTypeSection {
    let frame = curr_comp_frame(comp_stack);

    if frame.core_type_section.is_none() {
        frame.core_type_section = Some(
            CoreTypeSection::new()
        );
    }

    frame.core_type_section.as_mut().unwrap()
}

