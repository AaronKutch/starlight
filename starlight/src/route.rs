mod cedge;
mod channel;
mod cnode;
mod config;
#[cfg(feature = "debug")]
mod debug;
mod dilute;
mod embed;
mod path;
mod router;
mod routing;

#[allow(unused)]
use std::num::NonZeroU32;

use awint::awint_dag::triple_arena::ptr_struct;
pub use cedge::{CEdge, ChannelWidths, Programmability, SelectorLut, Source};
pub use channel::Channeler;
pub use cnode::CNode;
pub use config::{Config, Configurator};
pub(crate) use dilute::dilute_level;
pub use embed::{EdgeEmbed, NodeEmbed};
pub use path::{Edge, EdgeKind, HyperPath, NodeOrEdge, Path};
pub use router::Router;
pub(crate) use routing::route;

#[cfg(any(
    debug_assertions,
    all(feature = "gen_counters", not(feature = "u32_ptrs")),
))]
ptr_struct!(
    PCNode;
    PCEdge;
    PNodeEmbed;
    PEdgeEmbed;
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
    PNodeEmbed();
    PEdgeEmbed();
    PConfig();
    PMapping()
);

#[cfg(all(not(debug_assertions), feature = "gen_counters", feature = "u32_ptrs",))]
ptr_struct!(
    PCNode[NonZeroU32](NonZeroU32);
    PCEdge[NonZeroU32](NonZeroU32);
    PNodeEmbed[NonZeroU32](NonZeroU32);
    PEdgeEmbed[NonZeroU32](NonZeroU32);
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
    PNodeEmbed[NonZeroU32]();
    PEdgeEmbed[NonZeroU32]();
    PConfig[NonZeroU32]();
    PMapping[NonZeroU32]()
);

// these are completely internal and so can always go without gen counters

#[cfg(any(debug_assertions, not(feature = "u32_ptrs")))]
ptr_struct!(PBackToCnode());

#[cfg(all(not(debug_assertions), feature = "u32_ptrs"))]
ptr_struct!(PBackToCnode[NonZeroU32]());
