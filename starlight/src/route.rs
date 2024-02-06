mod cedge;
mod channel;
mod cnode;
mod config;
#[cfg(feature = "debug")]
mod debug;
mod embed;
mod path;
mod region_adv;
mod router;
mod routing;

pub use cedge::{BulkBehavior, CEdge, Programmability, SelectorLut, SelectorValue};
pub use channel::{Channeler, Referent};
pub use cnode::CNode;
pub use config::{Config, Configurator, PConfig};
pub use embed::{Embedding, EmbeddingKind, PEmbedding};
pub use path::{Edge, EdgeKind, HyperPath, Path};
pub use region_adv::RegionAdvancer;
pub use router::Router;
pub(crate) use routing::route;
