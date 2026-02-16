// I want this file to be a bunch of oneliners (easier to read)!

use crate::encode::component::collect::SubItemPlan;
use crate::encode::component::EncodeCtx;
use crate::ir::component::refs::{
    GetArgRefs, GetCompRefs, GetFuncRef, GetFuncRefs, GetItemRef, GetMemRefs, GetModuleRefs,
    GetTableRefs, GetTypeRefs,
};
use crate::ir::component::scopes::GetScopeKind;
use crate::ir::types::CustomSection;
use wasmparser::{
    ArrayType, CanonicalFunction, CanonicalOption, ComponentAlias, ComponentDefinedType,
    ComponentExport, ComponentFuncType, ComponentImport, ComponentInstance,
    ComponentInstantiationArg, ComponentStartFunction, ComponentType, ComponentTypeDeclaration,
    ComponentTypeRef, ComponentValType, CompositeInnerType, CompositeType, ContType, CoreType,
    Export, FieldType, FuncType, HeapType, Import, Instance, InstanceTypeDeclaration,
    InstantiationArg, ModuleTypeDeclaration, PackedIndex, PrimitiveValType, RecGroup, RefType,
    StorageType, StructType, SubType, TagType, TypeRef, UnpackedIndex, ValType, VariantCase,
};

mod sealed {
    pub trait Sealed {}
}
trait FixIndicesImpl {
    fn fixme(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self;
}
pub(crate) trait FixIndices: sealed::Sealed {
    fn fix(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self
    where
        Self: Sized;
}

impl<T> FixIndices for T
where
    T: GetScopeKind + sealed::Sealed + FixIndicesImpl,
{
    fn fix<'a>(&self, subitem_plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self
    where
        Self: Sized,
    {
        ctx.maybe_enter_scope(self);
        let fixed = self.fixme(subitem_plan, ctx);
        ctx.maybe_exit_scope(self);

        fixed
    }
}

impl sealed::Sealed for ComponentExport<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentExport<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let new_id = ctx.lookup_actual_id_or_panic(
            &self.get_item_ref().ref_
        );

        let fixed_ty = self.ty.map(|ty| {
            ty.fix(plan, ctx)
        });

        ComponentExport {
            name: self.name,
            kind: self.kind,
            index: new_id as u32,
            ty: fixed_ty,
        }
    }
}

impl sealed::Sealed for ComponentInstantiationArg<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentInstantiationArg<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let new_id = ctx.lookup_actual_id_or_panic(
            &self.get_item_ref().ref_
        );

        ComponentInstantiationArg {
            name: self.name,
            kind: self.kind,
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for ComponentType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentType::Defined(ty) => ComponentType::Defined(ty.fix(plan, ctx)),
            ComponentType::Func(ty) => ComponentType::Func(ty.fix(plan, ctx)),
            ComponentType::Component(tys) => {
                let mut new_tys = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &tys[*idx];
                    new_tys.push(decl.fix(subplan, ctx));
                }

                ComponentType::Component(new_tys.into_boxed_slice())
            },
            ComponentType::Instance(tys) => {
                let mut new_tys = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &tys[*idx];
                    new_tys.push(decl.fix(subplan, ctx));
                }

                ComponentType::Instance(new_tys.into_boxed_slice())
            },
            ComponentType::Resource { rep, dtor } => {
                ComponentType::Resource {
                    rep: rep.fix(plan, ctx),
                    dtor: dtor.map(|_| {
                        ctx.lookup_actual_id_or_panic(
                            &self.get_func_refs().first().unwrap().ref_
                        ) as u32
                    })
                }
            }
        }
    }
}

impl sealed::Sealed for ComponentInstance<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentInstance<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentInstance::Instantiate { args, .. } => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_comp_refs().first().unwrap().ref_
                );

                ComponentInstance::Instantiate {
                    component_index: new_id as u32,
                    args: args.iter().map( | arg| {
                        arg.fix(plan, ctx)
                    }).collect(),
                }
            }
            ComponentInstance::FromExports(export) => ComponentInstance::FromExports(
                export.iter().map(|value| {
                    value.fix(plan, ctx)
                }).collect()
            )
        }
    }
}

impl sealed::Sealed for CanonicalFunction {}
#[rustfmt::skip]
impl FixIndicesImpl for CanonicalFunction {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            CanonicalFunction::Lift { options: options_orig, .. } => {
                let new_fid = ctx.lookup_actual_id_or_panic(
                    &self.get_func_refs().first().unwrap().ref_
                );
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::Lift {
                    core_func_index: new_fid as u32,
                    type_index: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::Lower { options: options_orig, .. } => {
                let new_fid = ctx.lookup_actual_id_or_panic(
                    &self.get_func_refs().first().unwrap().ref_
                );
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::Lower {
                    func_index: new_fid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ResourceNew { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::ResourceNew { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDrop { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::ResourceDrop { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceRep { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::ResourceRep { resource: new_tid as u32}
            }
            CanonicalFunction::ResourceDropAsync { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::ResourceDropAsync { resource: new_tid as u32}
            }
            CanonicalFunction::TaskReturn {
                result,
                options: options_orig,
            } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::TaskReturn {
                    result: result.map(|v| {
                        v.fix(plan, ctx)
                    }),
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::WaitableSetWait { cancellable, .. } => {
                let new_mid = ctx.lookup_actual_id_or_panic(
                    &self.get_mem_refs().first().unwrap().ref_
                );

                CanonicalFunction::WaitableSetWait {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::WaitableSetPoll { cancellable, .. } => {
                let new_mid = ctx.lookup_actual_id_or_panic(
                    &self.get_mem_refs().first().unwrap().ref_
                );

                CanonicalFunction::WaitableSetPoll {
                    cancellable: *cancellable,
                    memory: new_mid as u32,
                }
            }
            CanonicalFunction::StreamNew { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::StreamNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamRead { options: options_orig, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::StreamRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamWrite { options: options_orig, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }

                CanonicalFunction::StreamWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::StreamCancelRead { async_, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::StreamCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamCancelWrite { async_, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::StreamCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureNew { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::FutureNew {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureRead { options: options_orig, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );


                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::FutureRead {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureWrite { options: options_orig, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );


                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::FutureWrite {
                    ty: new_tid as u32,
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::FutureCancelRead { async_, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::FutureCancelRead {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureCancelWrite { async_, .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::FutureCancelWrite {
                    async_: *async_,
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ErrorContextNew { options: options_orig } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::ErrorContextNew {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ErrorContextDebugMessage { options: options_orig } => {
                let mut fixed_options = vec![];
                for opt in options_orig.iter() {
                    fixed_options.push(opt.fix(plan, ctx));
                }
                CanonicalFunction::ErrorContextDebugMessage {
                    options: fixed_options.into_boxed_slice()
                }
            }
            CanonicalFunction::ThreadSpawnRef { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::ThreadSpawnRef {
                    func_ty_index: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadSpawnIndirect { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                let new_tbl_id = ctx.lookup_actual_id_or_panic(
                    &self.get_tbl_refs().first().unwrap().ref_
                );

                CanonicalFunction::ThreadSpawnIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::ThreadNewIndirect { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                let new_tbl_id = ctx.lookup_actual_id_or_panic(
                    &self.get_tbl_refs().first().unwrap().ref_
                );

                CanonicalFunction::ThreadNewIndirect {
                    func_ty_index: new_tid as u32,
                    table_index: new_tbl_id as u32,
                }
            }
            CanonicalFunction::StreamDropReadable { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::StreamDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::StreamDropWritable { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::StreamDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropReadable { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::FutureDropReadable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::FutureDropWritable { .. } => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                CanonicalFunction::FutureDropWritable {
                    ty: new_tid as u32,
                }
            }
            CanonicalFunction::ThreadAvailableParallelism
            | CanonicalFunction::BackpressureInc
            | CanonicalFunction::BackpressureDec
            | CanonicalFunction::WaitableSetNew
            | CanonicalFunction::WaitableSetDrop
            | CanonicalFunction::WaitableJoin
            | CanonicalFunction::SubtaskDrop
            | CanonicalFunction::TaskCancel
            | CanonicalFunction::SubtaskCancel { .. }
            | CanonicalFunction::ContextGet(_)
            | CanonicalFunction::ContextSet(_)
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

impl sealed::Sealed for Instance<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Instance<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            Instance::Instantiate { args: args_orig, .. } => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_module_refs().first().unwrap().ref_
                );

                let mut args = vec![];
                for arg in args_orig.iter() {
                    args.push(arg.fix(plan, ctx));
                }
                Instance::Instantiate {
                    module_index: new_id as u32,
                    args: args.into_boxed_slice()
                }
            }
            Instance::FromExports(exports_orig) => {
                let mut exports = vec![];
                for export in exports_orig.iter() {
                    exports.push(export.fix(plan, ctx));
                }
                Instance::FromExports(exports.into_boxed_slice())
            }
        }
    }
}

impl sealed::Sealed for ComponentStartFunction {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentStartFunction {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let new_fid = ctx.lookup_actual_id_or_panic(
            &self.get_func_ref().ref_
        );

        let mut new_args = vec![];
        for r in self.get_arg_refs().iter() {
            let new_arg = ctx.lookup_actual_id_or_panic(&r.ref_);
            new_args.push(new_arg as u32)
        }

        Self {
            func_index: new_fid as u32,
            arguments: new_args.into_boxed_slice(),
            results: self.results,
        }
    }
}

impl sealed::Sealed for CustomSection<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for CustomSection<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        self.clone()
    }
}

impl sealed::Sealed for ComponentDefinedType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentDefinedType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentDefinedType::Flags(_)
            | ComponentDefinedType::Enum(_) => self.clone(),
            ComponentDefinedType::Primitive(ty) => ComponentDefinedType::Primitive(ty.fix(plan, ctx)),
            ComponentDefinedType::Record(tys) => {
                let mut new_tys = vec![];
                for (s, ty) in tys.iter() {
                    new_tys.push((*s, ty.fix(plan, ctx)))
                }
                ComponentDefinedType::Record(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::Variant(tys) => {
                let mut new_tys = vec![];
                for ty in tys.iter() {
                    new_tys.push(ty.fix(plan, ctx))
                }
                ComponentDefinedType::Variant(new_tys.into_boxed_slice())
            },
            ComponentDefinedType::List(ty) => ComponentDefinedType::List(ty.fix(plan, ctx)),
            ComponentDefinedType::FixedSizeList(ty, len) => ComponentDefinedType::FixedSizeList(ty.fix(plan, ctx), *len),
            ComponentDefinedType::Tuple(tys) => {
                let mut new_tys = vec![];
                for t in tys.iter() {
                    new_tys.push(t.fix(plan, ctx))
                }
                ComponentDefinedType::Tuple(new_tys.into_boxed_slice())
            }
            ComponentDefinedType::Option(ty) => ComponentDefinedType::Option(ty.fix(plan, ctx)),
            ComponentDefinedType::Result { ok, err } => ComponentDefinedType::Result {
                ok: ok.as_ref().map(|ok| ok.fix(plan, ctx)),
                err: err.as_ref().map(|err| err.fix(plan, ctx))
            },
            ComponentDefinedType::Own(_) => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                ComponentDefinedType::Own(new_tid as u32)
            },
            ComponentDefinedType::Borrow(_) => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                ComponentDefinedType::Borrow(new_tid as u32)
            },
            ComponentDefinedType::Future(ty) => ComponentDefinedType::Future(ty.as_ref().map(|ty| ty.fix(plan, ctx))),
            ComponentDefinedType::Stream(ty) => ComponentDefinedType::Stream(ty.as_ref().map(|ty| ty.fix(plan, ctx))),
            ComponentDefinedType::Map(key_ty, val_ty) => ComponentDefinedType::Map(
                key_ty.fix(plan, ctx),
                val_ty.fix(plan, ctx)
            ),
        }
    }
}

impl sealed::Sealed for PrimitiveValType {}
#[rustfmt::skip]
impl FixIndicesImpl for PrimitiveValType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        *self
    }
}

impl sealed::Sealed for VariantCase<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for VariantCase<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            name: self.name,
            ty: self.ty.map(|ty| ty.fix(plan, ctx)),
            refines: self.refines.map(|_| {
                ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                ) as u32
            }),
        }
    }
}

impl sealed::Sealed for ComponentFuncType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentFuncType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_params = vec![];
        for (orig_name, orig_ty) in self.params.iter() {
            new_params.push((*orig_name, orig_ty.fix(plan, ctx)));
        }

        let new_res = self.result.map(|res| res.fix(plan, ctx));

        Self {
            async_: self.async_,
            params: new_params.into_boxed_slice(),
            result: new_res,
        }
    }
}

impl sealed::Sealed for SubType {}
#[rustfmt::skip]
impl FixIndicesImpl for SubType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            is_final: self.is_final,
            supertype_idx: if self.supertype_idx.is_some() {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                Some(PackedIndex::from_module_index(new_tid as u32).unwrap())
            } else {
                None
            },
            composite_type: self.composite_type.fix(plan, ctx)
        }
    }
}

impl sealed::Sealed for CompositeType {}
#[rustfmt::skip]
impl FixIndicesImpl for CompositeType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            inner: self.inner.fix(plan, ctx),
            shared: false,
            descriptor_idx: None,
            describes_idx: None,
        }
    }
}

impl sealed::Sealed for CompositeInnerType {}
#[rustfmt::skip]
impl FixIndicesImpl for CompositeInnerType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            CompositeInnerType::Func(ty) => CompositeInnerType::Func(ty.fix(plan, ctx)),
            CompositeInnerType::Array(ty) => CompositeInnerType::Array(ArrayType(ty.0.fix(plan, ctx))),
            CompositeInnerType::Struct(s) => CompositeInnerType::Struct(s.fix(plan, ctx)),
            CompositeInnerType::Cont(_) => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                CompositeInnerType::Cont(ContType(PackedIndex::from_module_index(new_tid as u32).unwrap()))
            },
        }
    }
}

impl sealed::Sealed for FuncType {}
#[rustfmt::skip]
impl FixIndicesImpl for FuncType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_params = vec![];
        for p in self.params() {
            new_params.push(p.fix(plan, ctx));
        }
        let mut new_results = vec![];
        for r in self.results() {
            new_results.push(r.fix(plan, ctx));
        }

        Self::new(new_params, new_results)
    }
}

impl sealed::Sealed for FieldType {}
#[rustfmt::skip]
impl FixIndicesImpl for FieldType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            element_type: self.element_type.fix(plan, ctx),
            mutable: self.mutable,
        }
    }
}

impl sealed::Sealed for StorageType {}
#[rustfmt::skip]
impl FixIndicesImpl for StorageType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            StorageType::I8
            | StorageType::I16 => *self,
            StorageType::Val(value) => StorageType::Val(value.fix(plan, ctx))
        }
    }
}

impl sealed::Sealed for StructType {}
#[rustfmt::skip]
impl FixIndicesImpl for StructType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let mut new_fields = vec![];
        for f in self.fields.iter() {
            new_fields.push(f.fix(plan, ctx));
        }

        Self {
            fields: new_fields.into_boxed_slice()
        }
    }
}

impl sealed::Sealed for ComponentTypeDeclaration<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentTypeDeclaration::CoreType(ty) => ComponentTypeDeclaration::CoreType(ty.fix(plan, ctx)),
            ComponentTypeDeclaration::Type(ty) => ComponentTypeDeclaration::Type(ty.fix(plan, ctx)),
            ComponentTypeDeclaration::Alias(a) => ComponentTypeDeclaration::Alias(a.fix(plan, ctx)),
            ComponentTypeDeclaration::Import(i) => ComponentTypeDeclaration::Import(i.fix(plan, ctx)),
            ComponentTypeDeclaration::Export { name, ty } => ComponentTypeDeclaration::Export {
                name: *name,
                ty: ty.fix(plan, ctx)
            },
        }
    }
}

impl sealed::Sealed for ValType {}
#[rustfmt::skip]
impl FixIndicesImpl for ValType {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ValType::I32
            | ValType::I64
            | ValType::F32
            | ValType::F64
            | ValType::V128 => *self,
            ValType::Ref(r) => ValType::Ref(r.fix(plan, ctx)),
        }
    }
}

impl sealed::Sealed for RefType {}
#[rustfmt::skip]
impl FixIndicesImpl for RefType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let refs = self.get_type_refs();
        if !refs.is_empty() {
            let new_heap = match self.heap_type() {
                HeapType::Concrete(_) => {
                    let new_tid = ctx.lookup_actual_id_or_panic(
                        &refs.first().unwrap().ref_
                    );
                    HeapType::Concrete(UnpackedIndex::Module(new_tid as u32))
                }

                HeapType::Exact(_) => {
                    let new_tid = ctx.lookup_actual_id_or_panic(
                        &refs.first().unwrap().ref_
                    );
                    HeapType::Exact(UnpackedIndex::Module(new_tid as u32))
                }

                HeapType::Abstract { .. } => {
                    // Abstract heap types never contain indices
                    return *self;
                }
            };

            Self::new(self.is_nullable(), new_heap).unwrap()
        } else {
            *self
        }
    }
}

impl sealed::Sealed for CoreType<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for CoreType<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match &self {
            CoreType::Rec(group) => {
                CoreType::Rec(group.fix(plan, ctx))
            }
            CoreType::Module(module) => {
                let mut new_modules = vec![];
                for (idx, subplan) in plan.as_ref().unwrap().order().iter() {
                    let decl = &module[*idx];
                    new_modules.push(decl.fix(subplan, ctx));
                }

                CoreType::Module(new_modules.into_boxed_slice())
            }
        }
    }
}

impl sealed::Sealed for ModuleTypeDeclaration<'_> {}
impl FixIndicesImpl for ModuleTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ModuleTypeDeclaration::Type(group) => ModuleTypeDeclaration::Type(group.fix(plan, ctx)),
            ModuleTypeDeclaration::Export { name, ty } => ModuleTypeDeclaration::Export {
                name,
                ty: ty.fix(plan, ctx),
            },
            ModuleTypeDeclaration::Import(import) => {
                ModuleTypeDeclaration::Import(import.fix(plan, ctx))
            }
            ModuleTypeDeclaration::OuterAlias { kind, count, .. } => {
                let new_tid =
                    ctx.lookup_actual_id_or_panic(&self.get_type_refs().first().unwrap().ref_);

                ModuleTypeDeclaration::OuterAlias {
                    kind: *kind,
                    count: *count,
                    index: new_tid as u32,
                }
            }
        }
    }
}

impl sealed::Sealed for Import<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Import<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            module: self.module,
            name: self.name,
            ty: self.ty.fix(plan, ctx),
        }
    }
}

impl sealed::Sealed for RecGroup {}
#[rustfmt::skip]
impl FixIndicesImpl for RecGroup {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, _: &mut EncodeCtx) -> Self {
        // NOTE: This is kept as an opaque IR node (indices not fixed here)
        // This is because wasmparser does not allow library users to create
        // a new RecGroup.
        // Indices will be fixed in `into_wasm_encoder_recgroup`!
        self.clone()
    }
}

impl sealed::Sealed for ComponentImport<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentImport<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        Self {
            name: self.name,
            ty: self.ty.fix(plan, ctx)
        }
    }
}

impl sealed::Sealed for ComponentValType {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentValType {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        if let ComponentValType::Type(_) = self {
            let new_tid = ctx.lookup_actual_id_or_panic(
                &self.get_type_refs().first().unwrap().ref_
            );
            ComponentValType::Type(new_tid as u32)
        } else {
            *self
        }
    }
}

impl sealed::Sealed for ComponentAlias<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentAlias<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentAlias::InstanceExport { kind, name, .. } => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_item_ref().ref_
                );

                Self::InstanceExport {
                    kind: *kind,
                    name,
                    instance_index: new_id as u32,
                }
            }
            ComponentAlias::CoreInstanceExport { kind, name, .. } => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_item_ref().ref_
                );

                Self::CoreInstanceExport {
                    kind: *kind,
                    name,
                    instance_index: new_id as u32,
                }
            }
            ComponentAlias::Outer { kind, count, .. } => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_item_ref().ref_
                );

                Self::Outer {
                    kind: *kind,
                    count: *count,
                    index: new_id as u32,
                }
            }
        }
    }
}

impl sealed::Sealed for ComponentTypeRef {}
#[rustfmt::skip]
impl FixIndicesImpl for ComponentTypeRef {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            ComponentTypeRef::Module(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                ComponentTypeRef::Module(new_id as u32)
            }
            ComponentTypeRef::Value(ty) => {
                ComponentTypeRef::Value(ty.fix(plan, ctx))
            }
            ComponentTypeRef::Func(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                ComponentTypeRef::Func(new_id as u32)
            }
            ComponentTypeRef::Instance(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                ComponentTypeRef::Instance(new_id as u32)
            }
            ComponentTypeRef::Component(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                ComponentTypeRef::Component(new_id as u32)
            }
            ComponentTypeRef::Type(_) => *self // nothing to do
        }
    }
}

impl sealed::Sealed for CanonicalOption {}
#[rustfmt::skip]
impl FixIndicesImpl for CanonicalOption {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            CanonicalOption::Realloc(_)
            | CanonicalOption::PostReturn(_)
            | CanonicalOption::Callback(_) => {
                let new_fid = ctx.lookup_actual_id_or_panic(
                    &self.get_func_refs().first().unwrap().ref_
                );

                match self {
                    CanonicalOption::Realloc(_) => CanonicalOption::Realloc(new_fid as u32),
                    CanonicalOption::PostReturn(_) => CanonicalOption::PostReturn(new_fid as u32),
                    CanonicalOption::Callback(_) => CanonicalOption::Callback(new_fid as u32),
                    _ => unreachable!(),
                }
            }
            CanonicalOption::CoreType(_) => {
                let new_tid = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                CanonicalOption::CoreType(new_tid as u32)
            }

            CanonicalOption::Memory(_) => {
                let new_mid = ctx.lookup_actual_id_or_panic(
                    &self.get_mem_refs().first().unwrap().ref_
                );
                CanonicalOption::Memory(new_mid as u32)
            }
            CanonicalOption::UTF8
            | CanonicalOption::UTF16
            | CanonicalOption::CompactUTF16
            | CanonicalOption::Async
            | CanonicalOption::Gc => *self
        }
    }
}

impl sealed::Sealed for InstantiationArg<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for InstantiationArg<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let new_id = ctx.lookup_actual_id_or_panic(
            &self.get_item_ref().ref_
        );
        Self {
            name: self.name,
            kind: self.kind,
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for Export<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for Export<'_> {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        let new_id = ctx.lookup_actual_id_or_panic(
            &self.get_item_ref().ref_
        );

        Self {
            name: self.name,
            kind: self.kind,
            index: new_id as u32,
        }
    }
}

impl sealed::Sealed for InstanceTypeDeclaration<'_> {}
#[rustfmt::skip]
impl FixIndicesImpl for InstanceTypeDeclaration<'_> {
    fn fixme<'a>(&self, plan: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            InstanceTypeDeclaration::CoreType(core_type) => InstanceTypeDeclaration::CoreType(core_type.fix(plan, ctx)),
            InstanceTypeDeclaration::Type(ty) => InstanceTypeDeclaration::Type(ty.fix(plan, ctx)),
            InstanceTypeDeclaration::Alias(alias) => InstanceTypeDeclaration::Alias(alias.fix(plan, ctx)),
            InstanceTypeDeclaration::Export { name, ty } => InstanceTypeDeclaration::Export {
                name: *name,
                ty: ty.fix(plan, ctx)
            },
        }
    }
}

impl sealed::Sealed for TypeRef {}
#[rustfmt::skip]
impl FixIndicesImpl for TypeRef {
    fn fixme<'a>(&self, _: &Option<SubItemPlan>, ctx: &mut EncodeCtx) -> Self {
        match self {
            TypeRef::Func(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );

                TypeRef::Func(new_id as u32)
            }
            TypeRef::Tag(TagType { kind, .. }) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                TypeRef::Tag(TagType {
                    kind: *kind,
                    func_type_idx: new_id as u32,
                })
            }
            TypeRef::FuncExact(_) => {
                let new_id = ctx.lookup_actual_id_or_panic(
                    &self.get_type_refs().first().unwrap().ref_
                );
                TypeRef::FuncExact(new_id as u32)
            }
            TypeRef::Table(_) | TypeRef::Memory(_) | TypeRef::Global(_) => *self
        }
    }
}
