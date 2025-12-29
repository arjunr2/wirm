use crate::Component;
use crate::encode::component::assign::assign_indices;
use crate::encode::component::collect::CollectCtx;
use crate::encode::component::encode::encode_internal;

mod collect;
mod assign;
pub(crate) mod encode;

/// Encoding a component gets split into 3 phases (the first two are for planning, the final
/// phase is to actually perform the encoding)
/// 1. Collect phase
///  - Walk dependencies
///  - Deduplicate using Seen
///  - Produce topologically sorted plan
/// 2. Index assignment phase
///  - Assign sequential indices after collection
///  - Separate from bytes
/// 3. Encoding phase
///  - Emit bytes using indices
///  - No recursion needed, all references are guaranteed to be valid
pub fn encode(comp: &Component) -> Vec<u8> {

    // Phase 1: Collect
    let mut ctx = CollectCtx::new(comp);
    comp.collect_root(&mut ctx);
    let mut plan = ctx.plan;
    let mut indices = ctx.indices;

    // Phase 2: Assign indices
    indices.reset_ids();
    assign_indices(&mut plan, &mut indices);

    // Phase 3: Encode (pass in the root-level component's plan, assigned indices, and original->new index map)
    let bytes = encode_internal(&comp, &plan, &indices);
    println!("{bytes:?}");

    bytes.finish()
}