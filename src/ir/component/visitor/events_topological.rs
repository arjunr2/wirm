use crate::Component;
use crate::ir::component::idx_spaces::IndexSpaceOf;
use crate::ir::component::section::ComponentSection;
use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::{ItemKind, VisitCtx};

pub(crate) fn get_topological_evts<'ir>(
    component: &'ir Component<'ir>,
    comp_idx: Option<usize>,
    ctx: &mut VisitCtx<'ir>,
    out: &mut Vec<VisitEvent<'ir>>,
) {
    todo!()
}
