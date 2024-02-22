use awint::{awint_dag::triple_arena::OrdArena, Awi};

use crate::{
    ensemble::{Ensemble, PBack, PExternal},
    epoch::get_current_epoch,
    route::{EdgeKind, EmbeddingKind, PConfig, Programmability, Router},
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
    /// Requires that the target epoch be resumed and is the active epoch
    pub fn config_target(&self) -> Result<(), Error> {
        todo!()
    }

    /*pub fn ensemble_config_target(&self, ensemble: &mut Ensemble) -> Result<(), Error> {
        Ok(())
    }*/

    /// Sets all the configurations derived from final embeddings
    pub(crate) fn set_configurations(&mut self) -> Result<(), Error> {
        // assumes that all config `value`s are set to `None` and we only route once,
        // otherwise we have to set them all to `None` at the start because it is used
        // to detect if there are contradictions

        for embedding in self.embeddings.vals() {
            match embedding.program {
                EmbeddingKind::Node(_) => {
                    // follow the `SelectorLut`s of the hyperpath
                    for path in embedding.target_hyperpath.paths() {
                        for edge in path.edges() {
                            match edge.kind {
                                EdgeKind::Transverse(q_cedge, source_i) => {
                                    let cedge = self.target_channeler.cedges.get(q_cedge).unwrap();
                                    match cedge.programmability() {
                                        // no-op with respect to configuration
                                        Programmability::TNode => (),
                                        // there are identity like cases where we might want to
                                        // traverse these kinds
                                        Programmability::StaticLut(_) => todo!(),
                                        Programmability::ArbitraryLut(_) => todo!(),
                                        Programmability::SelectorLut(selector_lut) => {
                                            let inx_config = selector_lut.inx_config();
                                            assert!(source_i < (1 << inx_config.len()));
                                            let i = Awi::from_usize(source_i);
                                            for (inx_i, p_config) in
                                                inx_config.iter().copied().enumerate()
                                            {
                                                let value = &mut self
                                                    .configurator
                                                    .configurations
                                                    .get_val_mut(p_config)
                                                    .unwrap()
                                                    .value;
                                                let desired_value = Some(i.get(inx_i).unwrap());
                                                if value.is_some() && (*value != desired_value) {
                                                    // means hyperpaths or base embeddings are
                                                    // conflicting
                                                    panic!(
                                                        "bug in router, a configuration bit has \
                                                         already been set and contradicts another \
                                                         desired configuration"
                                                    );
                                                }
                                                *value = desired_value;
                                            }
                                        }
                                        // the hyperpath should be fully lowered
                                        Programmability::Bulk(_) => unreachable!(),
                                    }
                                }
                                // the hyperpath should be fully lowered into base level traversals
                                EdgeKind::Concentrate | EdgeKind::Dilute => unreachable!(),
                            }
                        }
                    }
                }
                // need lowering to and configuration setting of `ArbitraryLut`s
                EmbeddingKind::Edge(_) => todo!(),
            }
        }

        Ok(())
    }
}

impl Default for Configurator {
    fn default() -> Self {
        Self::new()
    }
}
