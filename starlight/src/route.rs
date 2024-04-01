mod cedge;
mod channel;
mod cnode;
mod config;
#[cfg(feature = "debug")]
mod debug;
mod dilute;
mod embed;
mod hierarchy;
mod path;
mod router;
mod routing;

#[allow(unused)]
use std::num::NonZeroU32;

use awint::awint_dag::triple_arena::ptr_struct;
pub use cedge::{BulkProperties, MapPoint, Programmability, SelectorLut, Sink, Source};
pub use channel::Channeler;
pub use cnode::CNode;
pub use config::{Config, Configurator};
pub(crate) use dilute::dilute_level;
pub use embed::Embedding;
pub(crate) use hierarchy::generate_hierarchy;
pub use path::{Edge, HyperPath, Path};
pub use router::Router;
pub(crate) use routing::route;

#[cfg(any(
    debug_assertions,
    all(feature = "gen_counters", not(feature = "u32_ptrs")),
))]
ptr_struct!(
    PCNode;
    PEmbed;
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
    PEmbed();
    PConfig();
    PMapping()
);

#[cfg(all(not(debug_assertions), feature = "gen_counters", feature = "u32_ptrs",))]
ptr_struct!(
    PCNode[NonZeroU32](NonZeroU32);
    PEmbed[NonZeroU32](NonZeroU32);
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
    PEmbed[NonZeroU32]();
    PConfig[NonZeroU32]();
    PMapping[NonZeroU32]()
);

// these are completely internal and so can always go without gen counters

#[cfg(any(debug_assertions, not(feature = "u32_ptrs")))]
ptr_struct!(PMapPointToCnode());

#[cfg(all(not(debug_assertions), feature = "u32_ptrs"))]
ptr_struct!(PMapPointToCnode[NonZeroU32]());
