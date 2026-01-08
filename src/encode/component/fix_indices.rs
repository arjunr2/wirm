use wasm_encoder::Component;
use wasmparser::{ArrayType, CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType, ComponentExport, ComponentFuncType, ComponentImport, ComponentInstance, ComponentInstantiationArg, ComponentStartFunction, ComponentType, ComponentTypeDeclaration, ComponentTypeRef, ComponentValType, CompositeInnerType, CompositeType, ContType, CoreType, Export, FieldType, FuncType, HeapType, Import, Instance, InstanceTypeDeclaration, InstantiationArg, ModuleTypeDeclaration, PackedIndex, PrimitiveValType, RecGroup, RefType, StorageType, StructType, SubType, TagType, TypeRef, UnpackedIndex, ValType, VariantCase};
use crate::ir::component::idx_spaces::{IdxSpaces, ReferencedIndices};
use crate::ir::types::CustomSection;

pub(crate) trait FixIndices {
    fn fix<'a>(
        &self,
        component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self;
}

impl FixIndices for ComponentExport<'_> {
    fn fix<'a>(
        &self,
        component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);

        let fixed_ty = self.ty.map(|ty| {
            ty.fix(component, indices)
        });

        ComponentExport {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
            ty: fixed_ty,
        }
    }
}

impl FixIndices for ComponentInstantiationArg<'_> {
    fn fix<'a>(
        &self,
        _: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        let ty = refs.as_ref().unwrap().ty();
        let new_id = indices.lookup_actual_id_or_panic(&ty);

        ComponentInstantiationArg {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for ComponentType<'_> {
    fn fix<'a>(
        &self,
        component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        match self {
            ComponentType::Defined(ty) => ComponentType::Defined(ty.fix(component, indices)),
            ComponentType::Func(ty) => ComponentType::Func(ty.fix(component, indices)),
            ComponentType::Component(ty) => {
                let mut new_tys = vec![];
                for t in ty.iter() {
                    new_tys.push(t.fix(component, indices))
                }

                ComponentType::Component(new_tys.into_boxed_slice())
            },
            ComponentType::Instance(ty) => {
                let mut new_tys = vec![];
                for t in ty.iter() {
                    new_tys.push(t.fix(component, indices))
                }

                ComponentType::Instance(new_tys.into_boxed_slice())
            },
            ComponentType::Resource { rep, dtor } => {
                ComponentType::Resource {
                    rep: rep.fix(component, indices),
                    dtor: if dtor.is_some() {
                        let refs = self.referenced_indices();
                        let func = refs.as_ref().unwrap().func();
                        Some(indices.lookup_actual_id_or_panic(&func) as u32)
                    } else {
                        None
                    },
                }
            }
        }
    }
}

impl FixIndices for ComponentInstance<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            ComponentInstance::Instantiate { args, .. } => {
                let refs = self.referenced_indices();
                let comp = refs.as_ref().unwrap().comp();
                let new_id = indices.lookup_actual_id_or_panic(&comp);

                ComponentInstance::Instantiate {
                    component_index: new_id as u32,
                    args: args.iter().map( | arg| {
                        arg.fix(component, indices)
                    }).collect(),
                }
            }
            ComponentInstance::FromExports(export) => ComponentInstance::FromExports(
                export.iter().map(|value| {
                    value.fix(component, indices)
                }).collect()
            )
        }
    }
}

impl FixIndices for CanonicalFunction {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        let refs = self.referenced_indices();
        match self {
            CanonicalFunction::Lift {
                options: options_orig,
                ..
            } => {
                let func = refs.as_ref().unwrap().func();
                let ty = refs.as_ref().unwrap().ty();
                let new_fid = indices.lookup_actual_id_or_panic(&func);
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }

                CanonicalFunction::Lift {
                    core_func_index: new_fid as u32,
                    type_index: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::Lower {
                options: options_orig,
                ..
            } => {
                let func = refs.as_ref().unwrap().func();
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }

                let new_fid = indices.lookup_actual_id_or_panic(&func);
                CanonicalFunction::Lower {
                    func_index: new_fid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ResourceNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceNew { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDrop { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceDrop { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceRep { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceRep { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDropAsync { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ResourceDropAsync { resource: new_tid as u32}
            }
            CanonicalFunction::TaskReturn {
                result,
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }
                CanonicalFunction::TaskReturn {
                    result: result.map(|v| {
                        v.fix(component, indices)
                    }),
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::WaitableSetWait { cancellable, .. } => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                CanonicalFunction::WaitableSetWait {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::WaitableSetPoll { cancellable, .. } => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                CanonicalFunction::WaitableSetPoll {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::StreamNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamRead {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }

                CanonicalFunction::StreamRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamWrite {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }

                CanonicalFunction::StreamWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureNew { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureRead {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }
                CanonicalFunction::FutureRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureWrite {
                options: options_orig,
                ..
            } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }
                CanonicalFunction::FutureWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureCancelRead { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureCancelWrite { async_, .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ErrorContextNew {
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }
                CanonicalFunction::ErrorContextNew {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ErrorContextDebugMessage {
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(component, indices));
                }
                CanonicalFunction::ErrorContextDebugMessage {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ThreadSpawnRef { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::ThreadSpawnRef {
                    func_ty_index: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadSpawnIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = indices.lookup_actual_id_or_panic(&table);
                CanonicalFunction::ThreadSpawnIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::ThreadNewIndirect { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let table = refs.as_ref().unwrap().table();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                let new_tbl_id = indices.lookup_actual_id_or_panic(&table);
                CanonicalFunction::ThreadNewIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::StreamDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::StreamDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropReadable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropWritable { .. } => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalFunction::FutureDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureSet
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::ThreadYield { .. }
            | CanonicalFunction::ThreadIndex
            | CanonicalFunction::ThreadSwitchTo { .. }
            | CanonicalFunction::ThreadSuspend { .. }
            | CanonicalFunction::ThreadResumeLater
            | CanonicalFunction::ThreadYieldTo {..}
            | CanonicalFunction::ErrorContextDrop => self.clone(),
        }
    }
}

impl FixIndices for Instance<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            Instance::Instantiate {
                args: args_orig, ..
            } => {
                let refs = self.referenced_indices();
                let module = refs.as_ref().unwrap().module();
                let new_id = indices.lookup_actual_id_or_panic(&module);

                let mut args = vec![];
                for arg in args_orig.iter() {
                    args.push(arg.fix(component, indices));
                }
                Instance::Instantiate {
                    module_index: new_id as u32,
                    args: args.into_boxed_slice()
                }
            }
            Instance::FromExports(exports_orig) => {
                // NOTE: We will not be fixing ALL indices here (complexity)
                let mut exports = vec![];
                for export in exports_orig.iter() {
                    exports.push(export.fix(component, indices));
                }
                Instance::FromExports(exports.into_boxed_slice())
            }
        }
    }
}

impl FixIndices for ComponentStartFunction {
    fn fix<'a>(&self, _component: &mut Component, indices: &IdxSpaces) -> Self {
        let refs = self.referenced_indices();
        let func = refs.as_ref().unwrap().func();
        let new_fid = indices.lookup_actual_id_or_panic(&func);

        let mut new_args = vec![];
        for arg_refs in refs.as_ref().unwrap().others() {
            let ty = arg_refs.as_ref().unwrap().ty();
            let new_arg = indices.lookup_actual_id_or_panic(&ty);
            new_args.push(new_arg as u32)
        }

        Self {
            func_index: new_fid as u32,
            arguments: new_args.into_boxed_slice(),
            results: self.results,
        }
    }
}

impl FixIndices for CustomSection<'_> {
    fn fix<'a>(&self, _component: &mut Component, _indices: &IdxSpaces) -> Self {
        self.clone()
    }
}

impl FixIndices for ComponentDefinedType<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            ComponentDefinedType::Flags(_)
            | ComponentDefinedType::Enum(_) => self.clone(),
            ComponentDefinedType::Primitive(ty) => ComponentDefinedType::Primitive(ty.fix(component, indices)),
            ComponentDefinedType::Record(tys) => {
                let mut new_tys = vec![];
                for (s, ty) in tys.iter() {
                    new_tys.push((*s, ty.fix(component, indices)))
                }
                ComponentDefinedType::Record(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::Variant(tys) => {
                let mut new_tys = vec![];
                for ty in tys.iter() {
                    new_tys.push(ty.fix(component, indices))
                }
                ComponentDefinedType::Variant(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::List(ty) => ComponentDefinedType::List(ty.fix(component, indices)),
            ComponentDefinedType::FixedSizeList(ty, len) => ComponentDefinedType::FixedSizeList(ty.fix(component, indices), *len),
            ComponentDefinedType::Tuple(tys) => {
                let mut new_tys = vec![];
                for t in tys.iter() {
                    new_tys.push(t.fix(component, indices))
                }
                ComponentDefinedType::Tuple(new_tys.into_boxed_slice())
            }
            ComponentDefinedType::Option(ty) => ComponentDefinedType::Option(ty.fix(component, indices)),
            ComponentDefinedType::Result { ok, err } => ComponentDefinedType::Result {
                ok: if let Some(ok) = ok {
                    Some(ok.fix(component, indices))
                } else {
                    None
                },
                err: if let Some(err) = err {
                    Some(err.fix(component, indices))
                } else {
                    None
                }
            },
            ComponentDefinedType::Own(_) => {
                let refs = self.referenced_indices();
                let ty = refs.as_ref().unwrap().ty();
                let id = indices.lookup_actual_id_or_panic(&ty);
                ComponentDefinedType::Own(id as u32)
            },
            ComponentDefinedType::Borrow(_) => {
                let refs = self.referenced_indices();
                let ty = refs.as_ref().unwrap().ty();
                let id = indices.lookup_actual_id_or_panic(&ty);
                ComponentDefinedType::Borrow(id as u32)
            },
            ComponentDefinedType::Future(ty) => ComponentDefinedType::Future(if let Some(ty) = ty {
                Some(ty.fix(component, indices))
            } else {
                None
            }),
            ComponentDefinedType::Stream(ty) => ComponentDefinedType::Stream(if let Some(ty) = ty {
                Some(ty.fix(component, indices))
            } else {
                None
            }),
        }
    }
}

impl FixIndices for PrimitiveValType {
    fn fix<'a>(&self, _: &mut Component, _: &IdxSpaces) -> Self {
        self.clone()
    }
}

impl FixIndices for VariantCase<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            name: self.name,
            ty: self.ty.map(|ty| ty.fix(component, indices)),
            refines: self.refines.map(|_| {
                let refs = self.referenced_indices();
                let ty = refs.as_ref().unwrap().misc();
                indices.lookup_actual_id_or_panic(&ty) as u32
            }),
        }
    }
}

impl FixIndices for ComponentFuncType<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        let mut new_params = vec![];
        for (orig_name, orig_ty) in self.params.iter() {
            new_params.push((*orig_name, orig_ty.fix(component, indices)));
        }

        let new_res = if let Some(res) = self.result {
            Some(res.fix(component, indices))
        } else {
            None
        };

        Self {
            async_: self.async_,
            params: new_params.into_boxed_slice(),
            result: new_res,
        }
    }
}

impl FixIndices for SubType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            is_final: self.is_final,
            supertype_idx: if let Some(_) = self.supertype_idx {
                let refs = self.referenced_indices();
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                Some(PackedIndex::from_module_index(new_id as u32).unwrap())
            } else {
                None
            },
            composite_type: self.composite_type.fix(component, indices)
        }
    }
}

impl FixIndices for CompositeType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            inner: self.inner.fix(component, indices),
            shared: false,
            descriptor_idx: None,
            describes_idx: None,
        }
    }
}

impl FixIndices for CompositeInnerType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            CompositeInnerType::Func(ty) => CompositeInnerType::Func(ty.fix(component, indices)),
            CompositeInnerType::Array(ty) => CompositeInnerType::Array(ArrayType(ty.0.fix(component, indices))),
            CompositeInnerType::Struct(s) => CompositeInnerType::Struct(s.fix(component, indices)),
            CompositeInnerType::Cont(_) => {
                let refs = self.referenced_indices();
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                CompositeInnerType::Cont(ContType(PackedIndex::from_module_index(new_id as u32).unwrap()))
            },
        }
    }
}

impl FixIndices for FuncType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        let mut new_params = vec![];
        for p in self.params() {
            new_params.push(p.fix(component, indices));
        }
        let mut new_results = vec![];
        for r in self.results() {
            new_results.push(r.fix(component, indices));
        }

        Self::new(new_params, new_results)
    }
}

impl FixIndices for FieldType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            element_type: self.element_type.fix(component, indices),
            mutable: self.mutable,
        }
    }
}

impl FixIndices for StorageType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            StorageType::I8
            | StorageType::I16 => self.clone(),
            StorageType::Val(value) => StorageType::Val(value.fix(component, indices))
        }
    }
}

impl FixIndices for StructType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        let mut new_fields = vec![];
        for f in self.fields.iter() {
            new_fields.push(f.fix(component, indices));
        }

        Self {
            fields: new_fields.into_boxed_slice()
        }
    }
}

impl FixIndices for ComponentTypeDeclaration<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ComponentTypeDeclaration::CoreType(ty.fix(component, indices)),
            ComponentTypeDeclaration::Type(ty) => ComponentTypeDeclaration::Type(ty.fix(component, indices)),
            ComponentTypeDeclaration::Alias(a) => ComponentTypeDeclaration::Alias(a.fix(component, indices)),
            ComponentTypeDeclaration::Import(i) => ComponentTypeDeclaration::Import(i.fix(component, indices)),
            ComponentTypeDeclaration::Export { name, ty } => ComponentTypeDeclaration::Export {
                name: name.clone(),
                ty: ty.fix(component, indices)
            },
        }
    }
}

impl FixIndices for ValType {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            ValType::I32
            | ValType::I64
            | ValType::F32
            | ValType::F64
            | ValType::V128 => self.clone(),
            ValType::Ref(r) => ValType::Ref(r.fix(component, indices)),
        }
    }
}

impl FixIndices for RefType {
    fn fix<'a>(&self, _: &mut Component, indices: &IdxSpaces) -> Self {
        let refs = self.referenced_indices();
        let ty = refs.as_ref().unwrap().ty();
        let new_id = indices.lookup_actual_id_or_panic(&ty);

        // TODO -- there's no way this is correct...
        Self::new(self.is_nullable(), HeapType::Exact(UnpackedIndex::Module(new_id as u32))).unwrap()
    }
}

impl FixIndices for CoreType<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match &self {
            CoreType::Rec(recgroup) => {
                CoreType::Rec(recgroup.fix(component, indices))
            }
            CoreType::Module(module) => {
                let mut new_modules = vec![];
                for m in module.iter() {
                    new_modules.push(m.fix(component, indices));
                }
                CoreType::Module(new_modules.into_boxed_slice())
            }
        }
    }
}

impl FixIndices for ModuleTypeDeclaration<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        match self {
            ModuleTypeDeclaration::Type(group) => ModuleTypeDeclaration::Type(group.fix(component, indices)),
            ModuleTypeDeclaration::Export { name, ty } => ModuleTypeDeclaration::Export {
                name,
                ty: ty.fix(component, indices)
            },
            ModuleTypeDeclaration::Import(import) => ModuleTypeDeclaration::Import(import.fix(component, indices)),
            ModuleTypeDeclaration::OuterAlias { .. } => self.clone(), // TODO: Fix this after scoped index spaces!
        }
    }
}

impl FixIndices for Import<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            module: self.module,
            name: self.name,
            ty: self.ty.fix(component, indices),
        }
    }
}

impl FixIndices for RecGroup {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        // I can't do this structure for RecGroup (unable to construct outside the wasmparser library)
        // Need to do something special to handle this :/
        todo!()
    }
}

impl FixIndices for ComponentImport<'_> {
    fn fix<'a>(&self, component: &mut Component, indices: &IdxSpaces) -> Self {
        Self {
            name: self.name,
            ty: self.ty.fix(component, indices)
        }
    }
}

impl FixIndices for ComponentValType {
    fn fix<'a>(
        &self,
        _component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        if let ComponentValType::Type(_) = self {
            let refs = self.referenced_indices();
            let ty = refs.as_ref().unwrap().ty();
            let new_id = indices.lookup_actual_id_or_panic(&ty);
            ComponentValType::Type(new_id as u32)
        } else {
            self.clone()
        }
    }
}

impl FixIndices for ComponentAlias<'_> {
    fn fix<'a>(
        &self,
        _component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        match self {
            ComponentAlias::InstanceExport { kind, name, .. } => {
                let refs = self.referenced_indices();
                let inst = refs.as_ref().unwrap().inst();
                let new_id = indices.lookup_actual_id_or_panic(&inst);
                Self::InstanceExport {
                    kind: kind.clone(),
                    name,
                    instance_index: new_id as u32,
                }
            }
            ComponentAlias::CoreInstanceExport { kind, name, .. } => {
                let refs = self.referenced_indices();
                let inst = refs.as_ref().unwrap().inst();
                let new_id = indices.lookup_actual_id_or_panic(&inst);
                Self::CoreInstanceExport {
                    kind: kind.clone(),
                    name,
                    instance_index: new_id as u32,
                }
            }
            ComponentAlias::Outer { .. } => self.clone(), // TODO: Fix this after scoped index spaces!
        }
    }
}

impl FixIndices for ComponentTypeRef {
    fn fix<'a>(
        &self,
        component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            ComponentTypeRef::Module(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Module(new_id as u32)
            }
            ComponentTypeRef::Value(ty) => {
                ComponentTypeRef::Value(ty.fix(component, indices))
            }
            ComponentTypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Func(new_id as u32)
            }
            ComponentTypeRef::Instance(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Instance(new_id as u32)
            }
            ComponentTypeRef::Component(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                ComponentTypeRef::Component(new_id as u32)
            }
            ComponentTypeRef::Type(_) => self.clone(), // nothing to do
        }
    }
}

impl FixIndices for CanonicalOption {
    fn fix<'a>(
        &self,
        _: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Callback(_) => {
                let func = refs.as_ref().unwrap().func();
                let new_fid = indices.lookup_actual_id_or_panic(&func);
                match self {
                    CanonicalOption::Realloc(_) => CanonicalOption::Realloc(new_fid as u32),
                    CanonicalOption::PostReturn(_) => CanonicalOption::PostReturn(new_fid as u32),
                    CanonicalOption::Callback(_) => CanonicalOption::Callback(new_fid as u32),
                    _ => unreachable!(),
                }
            }
            CanonicalOption::CoreType(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_tid = indices.lookup_actual_id_or_panic(&ty);
                CanonicalOption::CoreType(new_tid as u32)
            }

            CanonicalOption::Memory(_) => {
                let mem = refs.as_ref().unwrap().mem();
                let new_mid = indices.lookup_actual_id_or_panic(&mem);
                CanonicalOption::Memory(new_mid as u32)
            }
            CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Async
            | CanonicalOption::Gc => self.clone(),
        }
    }
}

impl FixIndices for InstantiationArg<'_> {
    fn fix<'a>(
        &self,
        _component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for Export<'_> {
    fn fix<'a>(
        &self,
        _component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        let misc = refs.as_ref().unwrap().misc();
        let new_id = indices.lookup_actual_id_or_panic(&misc);
        Self {
            name: self.name,
            kind: self.kind.clone(),
            index: new_id as u32,
        }
    }
}

impl FixIndices for InstanceTypeDeclaration<'_> {
    fn fix<'a>(
        &self,
        component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        match self {
            InstanceTypeDeclaration::CoreType(core_type) => InstanceTypeDeclaration::CoreType(core_type.fix(component, indices)),
            InstanceTypeDeclaration::Type(ty) => InstanceTypeDeclaration::Type(ty.fix(component, indices)),
            InstanceTypeDeclaration::Alias(alias) => InstanceTypeDeclaration::Alias(alias.fix(component, indices)),
            InstanceTypeDeclaration::Export { name, ty } => InstanceTypeDeclaration::Export {
                name: name.clone(),
                ty: ty.fix(component, indices)
            },
        }
    }
}

impl FixIndices for TypeRef {
    fn fix<'a>(
        &self,
        _component: &mut Component,
        indices: &IdxSpaces,
    ) -> Self {
        let refs = self.referenced_indices();
        match self {
            TypeRef::Func(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::Func(new_id as u32)
            }
            TypeRef::Tag(TagType {
                             kind,
                             func_type_idx: _,
                         }) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::Tag(TagType {
                    kind: kind.clone(),
                    func_type_idx: new_id as u32,
                })
            }
            TypeRef::FuncExact(_) => {
                let ty = refs.as_ref().unwrap().ty();
                let new_id = indices.lookup_actual_id_or_panic(&ty);
                TypeRef::FuncExact(new_id as u32)
            }
            TypeRef::Table(_) | TypeRef::Memory(_) | TypeRef::Global(_) => self.clone(),
        }
    }
}