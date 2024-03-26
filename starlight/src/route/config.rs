use std::num::{NonZeroU64, NonZeroUsize};

use awint::{
    awint_dag::triple_arena::{OrdArena, Ptr},
    Awi, Bits,
};

use crate::{
    ensemble::{Ensemble, PExternal, Value},
    epoch::get_current_epoch,
    route::{PConfig, Router},
    Error, LazyAwi,
};

/// The comparison traits are only implemented on the `p_external`
#[derive(Debug, Clone)]
pub struct Config {
    /// The bit value the configuration wants
    value: Awi,
    /// Bits of this are only set if the corresponding value bit is known
    value_known: Awi,
}

impl Config {
    /// Create a new `Config` with `p_external`, bitwidth `w` that should
    /// correspond to the width of the `RNode` pointed to by `p_external`, and
    /// sets all bit values to unknown
    pub fn new(w: NonZeroUsize) -> Self {
        Self {
            value: Awi::zero(w),
            value_known: Awi::zero(w),
        }
    }

    pub fn nzbw(&self) -> NonZeroUsize {
        self.value.nzbw()
    }

    /// Returns the bits that the configurable should be set to
    pub fn value(&self) -> &Bits {
        &self.value
    }

    /// Returns corresponding bits to `value` that indicate if the value is
    /// known/necessary
    pub fn value_known(&self) -> &Bits {
        &self.value_known
    }

    pub fn value_mut(&mut self) -> &mut Bits {
        &mut self.value
    }

    pub fn value_known_mut(&mut self) -> &mut Bits {
        &mut self.value_known
    }
}

/// The channeler for the target needs to know which bits the router can use to
/// configure different behaviors.
#[derive(Debug, Clone)]
pub struct Configurator {
    // `PEquiv` to `PExternal` mapping for bits we are allowed to configure
    pub configurations: OrdArena<PConfig, PExternal, Config>,
}

impl Configurator {
    pub fn new() -> Self {
        Self {
            configurations: OrdArena::new(),
        }
    }

    /// Tell the router what bits it can use for programming the target. Uses
    /// the current `Epoch`.
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
        let p_external = config.borrow().p_external();
        // check for existence in ensemble
        let (_, rnode) = ensemble.notary.get_rnode(p_external)?;
        let config_struct = Config::new(rnode.nzbw());
        // TODO add something to `triple_arena` to emulate this
        if let Some((p_config, dir)) = self.configurations.find_similar_key(&p_external) {
            if dir.is_eq() {
                return Err(Error::OtherString(format!(
                    "`configurable({p_external:#?})`: found that `configurable` was called twice \
                     on the same `LazyAwi`"
                )));
            } else {
                self.configurations.insert_inx_manual_unwrap(
                    p_config.inx(),
                    dir,
                    p_external,
                    config_struct,
                );
            }
        } else {
            self.configurations.insert_empty(p_external, config_struct);
        }
        Ok(())
    }
}

impl Router {
    /// Finds the configuration associated with `config`. Does _not_
    /// require that the target epoch be the current epoch.
    ///
    /// # Errors
    ///
    /// - If the routing is invalid because it has never been successfully
    ///   routed or has been invalidated because of changes.
    /// - If `config` was not registered in the `Configurator` used for the
    ///   routing
    pub fn get_config<L: std::borrow::Borrow<LazyAwi>>(&self, config: &L) -> Result<Config, Error> {
        if !self.is_valid_routing {
            return Err(Error::RoutingIsInvalid)
        }
        let p_external = config.borrow().p_external();

        if let Some(p_config) = self.configurator.configurations.find_key(&p_external) {
            if let Ok((_, rnode)) = self.target_ensemble().notary.get_rnode(p_external) {
                let config = self.configurator.configurations.get_val(p_config).unwrap();
                if config.nzbw() == rnode.nzbw() {
                    Ok(config.clone())
                } else {
                    Err(Error::OtherStr(
                        "`get_config({p_external:#?})`: bitwidth mismatch, the target must have \
                         been improperly mutated",
                    ))
                }
            } else {
                Err(Error::InvalidPExternalConfig(p_external))
            }
        } else {
            Err(Error::OtherStr(
                "`get_config({p_external:#?})`: `config` is not registered as configurable in the \
                 configurator",
            ))
        }
    }

    /// Iterates through all of the configurable bits from the `Configurator`
    /// and sets them in the target `Epoch`.
    ///
    /// # Errors
    ///
    /// - If the routing is invalid because it has never been successfully
    ///   routed or has been invalidated because of changes.
    /// - If the target epoch is not the current `Epoch`
    pub fn config_target(&self) -> Result<(), Error> {
        if !self.is_valid_routing {
            return Err(Error::RoutingIsInvalid)
        }
        let epoch_shared = get_current_epoch()?;
        let mut lock = epoch_shared.epoch_data.borrow_mut();
        let ensemble = &mut lock.ensemble;
        self.ensemble_config_target(ensemble)
    }

    /// Iterates through all of the configurable bits from the `Configurator`
    /// and sets them in the target `Ensemble`.
    ///
    /// # Errors
    ///
    /// - If the routing is invalid because it has never been successfully
    ///   routed or has been invalidated because of changes.
    /// - If the `ensemble` is not the target ensemble
    pub fn ensemble_config_target(&self, ensemble: &mut Ensemble) -> Result<(), Error> {
        if !self.is_valid_routing {
            return Err(Error::RoutingIsInvalid)
        }
        for (_, p_external, config) in &self.configurator.configurations {
            let p_external = *p_external;
            // check that we are in the right epoch, the `p_equiv` lookup could collide
            if let Ok((p_rnode, rnode)) = ensemble.notary.get_rnode(p_external) {
                if config.nzbw() == rnode.nzbw() {
                    if let Some(bits) = ensemble.notary.rnodes[p_rnode].bits() {
                        for bit_i in 0..bits.len() {
                            let p_back = ensemble.notary.rnodes[p_rnode].bits().unwrap()[bit_i];
                            if let Some(p_back) = p_back {
                                let bit = config.value().get(bit_i).unwrap();
                                let known = config.value_known().get(bit_i).unwrap();
                                let value = if known {
                                    Value::Dynam(bit)
                                } else {
                                    Value::Unknown
                                };
                                let p_equiv = ensemble.get_p_equiv(p_back).unwrap();
                                ensemble.change_value(
                                    p_equiv,
                                    value,
                                    NonZeroU64::new(1).unwrap(),
                                )?;
                            }
                            // else the bit was optimized away normally
                        }
                    } else {
                        // this shouldn't be encountered after the whole routing process
                        return Err(Error::OtherString(format!(
                            "`config_target`: when trying to change the target bits corresponding \
                             to {p_external:#?}, encountered problem that may be due to improper \
                             target mutation: found uninitialized `RNode` bits"
                        )))
                    }
                } else {
                    return Err(Error::OtherString(format!(
                        "`config_target`: when trying to change the target bit corresponding to \
                         {p_external:#?}, encountered bitwidth mismatch that may be due to \
                         improper target mutation"
                    )))
                }
            } else {
                return Err(Error::NotInTargetEpoch);
            }
        }
        Ok(())
    }

    /// Sets all the configurations derived from final embeddings
    pub(crate) fn set_configurations(&mut self) -> Result<(), Error> {
        // need to clear all in case of reroute, the `value_known` state is used for
        // detecting contradictions
        for config in self.configurator.configurations.vals_mut() {
            config.value_known_mut().zero_();
        }

        todo!();
        /*
        for embedding in self.embeddings.vals() {
            // follow the `SelectorLut`s of the hyperpath
            for path in embedding.hyperpath.paths() {
                for edge in path.edges() {
                    match edge.kind {
                        EdgeKind::Transverse(q_cedge, source_i) => {
                            let cedge = self.target_channeler.cedges.get(q_cedge).unwrap();
                            match cedge.programmability() {
                                // there are identity like cases where we might want to
                                // traverse these kinds
                                Programmability::StaticLut(_) => todo!(),
                                Programmability::ArbitraryLut(_) => todo!(),
                                Programmability::SelectorLut(selector_lut) => {
                                    let inx_config = selector_lut.inx_config();
                                    assert!(source_i < (1 << inx_config.len()));
                                    let i = Awi::from_usize(source_i);
                                    for (inx_i, p_config) in inx_config.iter().copied().enumerate()
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
                                                "bug in router, a configuration bit has already \
                                                 been set and contradicts another desired \
                                                 configuration"
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
        */

        Ok(())
    }
}

impl Default for Configurator {
    fn default() -> Self {
        Self::new()
    }
}
