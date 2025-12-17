// Phase 3

use wasmparser::CanonicalFunction;
use crate::encode::component::assign::Indices;
use crate::encode::component::collect::{EncodeItem, EncodePlan};

fn encode<'a>(plan: &EncodePlan<'a>, indices: &Indices) -> Vec<u8> {
    let mut out = Vec::new();
    for item in &plan.items {
        match item {
            EncodeItem::CanonicalFunc(f) => f.encode(indices, &mut out),
            // EncodeItem::Module(m) => m.encode(&indices, &mut out),
            // EncodeItem::Component(c) => c.encode(&indices, &mut out),
            // EncodeItem::Type(ty) => ty.encode(&indices, &mut out),
        }
    }

    // for func in &plan.funcs {
    //     let idx = indices.canonical_func[&(*func as *const _)];
    //     out.push(idx as u8); // pretend the "encoding" is just the index
    //     // encode body etc.
    // }
    out
}


trait Encode {
    fn encode<'a>(&self, indices: &Indices, out: &mut [u8]);
}

impl Encode for CanonicalFunction {
    fn encode<'a>(&self, indices: &Indices, out: &mut [u8]) {
        todo!()
    }
}
