mod cedge;
mod channel;
mod cnode;
mod config;
#[cfg(feature = "debug")]
mod debug;
mod embed;
mod path;
mod router;
mod routing;

#[allow(unused)]
use std::num::NonZeroU32;

use awint::awint_dag::triple_arena::ptr_struct;
pub use cedge::{CEdge, ChannelWidths, Programmability, SelectorLut, SelectorValue};
pub use channel::{Channeler, Referent};
pub use cnode::CNode;
pub use config::{Config, Configurator};
pub use embed::{Embedding, EmbeddingKind};
pub use path::{Edge, EdgeKind, HyperPath, Path};
pub use router::Router;
pub(crate) use routing::route;

#[cfg(any(
    debug_assertions,
    all(feature = "gen_counters", not(feature = "u32_ptrs")),
))]
ptr_struct!(
    PCNode;
    PCEdge;
    QCNode;
    QCEdge;
    PEmbedding;
    PConfig;
    PMapping
);

#[cfg(all(
    not(debug_assertions),
    not(feature = "gen_counters"),
    not(feature = "u32_ptrs"),
))]
ptr_struct!(
    PCNode();
    PCEdge();
    QCNode();
    QCEdge();
    PEmbedding();
    PConfig();
    PMapping()
);

#[cfg(all(not(debug_assertions), feature = "gen_counters", feature = "u32_ptrs",))]
ptr_struct!(
    PCNode[NonZeroU32](NonZeroU32);
    PCEdge[NonZeroU32](NonZeroU32);
    QCNode[NonZeroU32](NonZeroU32);
    QCEdge[NonZeroU32](NonZeroU32);
    PEmbedding[NonZeroU32](NonZeroU32);
    PConfig[NonZeroU32](NonZeroU32);
    PMapping[NonZeroU32](NonZeroU32)
);

#[cfg(all(
    not(debug_assertions),
    not(feature = "gen_counters"),
    feature = "u32_ptrs",
))]
ptr_struct!(
    PCNode[NonZeroU32]();
    PCEdge[NonZeroU32]();
    QCNode[NonZeroU32]();
    QCEdge[NonZeroU32]();
    PEmbedding[NonZeroU32]();
    PConfig[NonZeroU32]();
    PMapping[NonZeroU32]()
);

// these are completely internal and so can always go without gen counters

#[cfg(any(debug_assertions, not(feature = "u32_ptrs")))]
ptr_struct!(PUniqueCNode(); PBackrefToBackref(); PTopLevel());

#[cfg(all(not(debug_assertions), feature = "u32_ptrs"))]
ptr_struct!(PUniqueCNode[NonZeroU32](); PBackrefToBackref[NonZeroU32](); PTopLevel[NonZeroU32]());
