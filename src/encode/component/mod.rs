use crate::Component;
use crate::encode::component::assign::assign_indices;
use crate::encode::component::collect::CollectCtx;
use crate::encode::component::encode::encode_internal;

mod collect;
mod assign;
mod encode;

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
    let mut ctx = CollectCtx::default();
    comp.collect_root(&mut ctx);
    let mut plan = ctx.plan;

    // Phase 2: Assign indices
    let (indices, map) = assign_indices(&mut plan);

    // Phase 3: Encode (pass in the root-level component's plan, assigned indices, and original->new index map)
    let bytes = encode_internal(&plan, &indices, &map);
    println!("{bytes:?}");

    bytes.finish()
}