use awint::awint_dag::triple_arena::{
    DirectArena, SimpleOrdArena, SurjectArena,
    traits::*,
    utils::traits::{ArenaBacking, PtrGen},
};

// (This would be a standard function, except there are far too many choices to
// make on the backing of the recaster arena and how fallibility should be
// handled)
pub fn compress_recaster<P: Ptr, T, A: CompactArenaTrait<P, T>>(
    this: &mut A,
    reset_generation: bool,
) -> DirectArena<P, P> {
    // this arena will be a recaster in which we create a mapping from the old `Ptr`
    // domain to the new one
    let mut res = DirectArena::<P, P>::new();
    // this sets all the keys of the mapping by cloning the `Ptr` validities of the
    // pre-compression `self` into the recaster and puts in invalid placeholders for
    // the new domain
    res.clone_from_with(this, |_, _| P::invalid()).unwrap();
    // compress and write the new `Ptr`s at the indexes of the corresponding old
    // ones, completing the mapping
    this.compress_with(reset_generation, |p, _, q| *res.get_mut(p).unwrap() = q)
        .allow();
    res
}

pub fn ord_arena_canonical_compress_recaster<P: Ptr, T, B: ArenaBacking>(
    this: &mut SimpleOrdArena<P, T, B>,
    reset_generation: bool,
) -> DirectArena<P, P> {
    // this arena will be a recaster in which we create a mapping from the old `Ptr`
    // domain to the new one
    let mut recaster = DirectArena::<P, P>::new();
    recaster.clone_from_with(this, |_, _| P::invalid()).unwrap();
    let mut replacement = SimpleOrdArena::new();
    let generation = if reset_generation {
        <P as Ptr>::Gen::two()
    } else {
        this.inc_generation().allow();
        this.generation()
    };
    replacement
        .transfer_canonical_reallocating(generation, this, |q, t, p| {
            recaster[q] = p;
            t.allow()
        })
        .unwrap();
    *this = replacement;
    recaster
}

pub fn surject_arena_canonical_compress_recaster<P: Ptr, K, V, B: ArenaBacking>(
    this: &mut SurjectArena<P, K, V, B>,
    reset_generation: bool,
) -> DirectArena<P, P> {
    // this arena will be a recaster in which we create a mapping from the old `Ptr`
    // domain to the new one
    let mut recaster = DirectArena::<P, P>::new();
    recaster.reallocate_min_capacity(this.len()).unwrap();
    let mut replacement = SurjectArena::new();
    let generation = if reset_generation {
        <P as Ptr>::Gen::two()
    } else {
        this.inc_generation().allow();
        this.generation()
    };
    replacement
        .transfer_canonical_reallocating(
            generation,
            this,
            |q, k, p| {
                recaster.direct_insert_within_capacity(q).unwrap().insert(p);
                k.allow()
            },
            |v| v,
        )
        .unwrap();
    *this = replacement;
    recaster
}
