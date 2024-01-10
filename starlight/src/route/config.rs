use awint::awint_dag::triple_arena::{ptr_struct, OrdArena};

use crate::{
    ensemble::{self, Ensemble, PExternal},
    Error, LazyAwi,
};

ptr_struct!(PConfig);

#[derive(Debug, Clone)]
pub struct Config {
    /// stable `Ptr` for the target
    p_external: PExternal,
    /// The index in the `RNode`
    bit_i: usize,
    /// The bit value the configuration wants. `None` is for not yet determined
    /// or for if the value can be set to `Value::Unknown`.
    value: Option<bool>,
}

/// The channeler for the target needs to know which bits the router can use to
/// configure different behaviors.
#[derive(Debug, Clone)]
pub struct Configurator {
    // `ThisEquiv` `PBack` to `PExternal` mapping for bits we are allowed to configure
    pub configurations: OrdArena<PConfig, ensemble::PBack, Config>,
}

impl Configurator {
    pub fn new() -> Self {
        Self {
            configurations: OrdArena::new(),
        }
    }

    pub fn find(&self, p_equiv: ensemble::PBack) -> Option<PConfig> {
        self.configurations.find_key(&p_equiv)
    }

    /// Tell the router what bits it can use for programming the target
    pub fn make_configurable(
        &mut self,
        ensemble: &Ensemble,
        config: &LazyAwi,
    ) -> Result<(), Error> {
        let p_external = config.p_external();
        if let Some((_, rnode)) = ensemble.notary.get_rnode(p_external) {
            for (bit_i, bit) in rnode.bits.iter().enumerate() {
                if let Some(bit) = bit {
                    let p_equiv = ensemble.backrefs.get_val(*bit).unwrap().p_self_equiv;
                    let (_, replaced) = self.configurations.insert(p_equiv, Config {
                        p_external,
                        bit_i,
                        value: None,
                    });
                    // we may want to allow this, if we have a mechanism to make sure they are set
                    // to the same thing
                    if replaced.is_some() {
                        return Err(Error::OtherString(format!(
                            "`make_configurable(.., {config:?})`: found that the same bit as a \
                             previous one is configurable, this may be because \
                             `make_configurable` was called twice on the same or equivalent bit"
                        )));
                    }
                }
            }
            Ok(())
        } else {
            Err(Error::OtherString(format!(
                "`make_configurable(.., {config:?})`: could not find the `config` in the \
                 `Ensemble` (probably, you are using something from the program ensemble instead \
                 of the target ensemble)"
            )))
        }
    }
}

impl Default for Configurator {
    fn default() -> Self {
        Self::new()
    }
}
