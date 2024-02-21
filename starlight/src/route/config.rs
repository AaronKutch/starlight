use awint::awint_dag::triple_arena::OrdArena;

use super::PConfig;
use crate::{
    ensemble::{Ensemble, PBack, PExternal},
    epoch::get_current_epoch,
    route::Router,
    Error, LazyAwi,
};

#[derive(Debug, Clone)]
pub struct Config {
    /// stable `Ptr` for the target
    pub p_external: PExternal,
    /// The index in the `RNode`
    pub bit_i: usize,
    /// The bit value the configuration wants. `None` is for not yet determined
    /// or for if the value can be set to `Value::Unknown`.
    pub value: Option<bool>,
}

/// The channeler for the target needs to know which bits the router can use to
/// configure different behaviors.
#[derive(Debug, Clone)]
pub struct Configurator {
    // `ThisEquiv` `PBack` to `PExternal` mapping for bits we are allowed to configure
    pub configurations: OrdArena<PConfig, PBack, Config>,
}

impl Configurator {
    pub fn new() -> Self {
        Self {
            configurations: OrdArena::new(),
        }
    }

    pub fn find(&self, p_equiv: PBack) -> Option<PConfig> {
        self.configurations.find_key(&p_equiv)
    }

    /// Tell the router what bits it can use for programming the target. Uses
    /// the currently active `Epoch`.
    pub fn configurable<L: std::borrow::Borrow<LazyAwi>>(
        &mut self,
        config: &L,
    ) -> Result<(), Error> {
        let epoch_shared = get_current_epoch()?;
        let lock = epoch_shared.epoch_data.borrow();
        let ensemble = &lock.ensemble;
        self.ensemble_make_configurable(ensemble, config)
    }

    /// Tell the router what bits it can use for programming the target
    pub fn ensemble_make_configurable<L: std::borrow::Borrow<LazyAwi>>(
        &mut self,
        ensemble: &Ensemble,
        config: &L,
    ) -> Result<(), Error> {
        let config = config.borrow();
        let p_external = config.p_external();
        let (_, rnode) = ensemble.notary.get_rnode(p_external)?;
        if let Some(bits) = rnode.bits() {
            for (bit_i, bit) in bits.iter().copied().enumerate() {
                if let Some(bit) = bit {
                    let p_equiv = ensemble.backrefs.get_val(bit).unwrap().p_self_equiv;
                    let (_, replaced) = self.configurations.insert(p_equiv, Config {
                        p_external,
                        bit_i,
                        value: None,
                    });
                    // we may want to allow this, if we have a mechanism to make sure they are
                    // set to the same thing
                    if replaced.is_some() {
                        return Err(Error::OtherString(format!(
                            "`configurable({config:#?})`: found that the same bit as a previous \
                             one is configurable, this may be because `configurable` was called \
                             twice on the same or equivalent bit"
                        )));
                    }
                }
            }
        } else {
            return Err(Error::OtherStr(
                "`configurable({config:#?})`: found that the epoch has not been lowered and \
                 preferably optimized",
            ));
        }
        Ok(())
    }
}

impl Router {
    /// Sets all the configurations derived from final embeddings
    pub(crate) fn set_configurations(&mut self) -> Result<(), Error> {
        //
        Ok(())
    }
}

impl Default for Configurator {
    fn default() -> Self {
        Self::new()
    }
}
