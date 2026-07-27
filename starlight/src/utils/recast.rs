use awint::awint_dag::triple_arena::{Arena, traits::*};

// (This would be a standard function, except there are far too many choices to
// make on the backing of the recaster arena and how fallibility should be
// handled)
pub fn compress_recaster<
    P: Ptr,
    T,
    A: ArenaTrait<P, T> + SingularGenerationArena<P> + ArenaCloneFromWith<P, T>,
>(
    this: &mut A,
    reset_generation: bool,
) -> Arena<P, P> {
    // this arena will be a recaster in which we create a mapping from the old `Ptr`
    // domain to the new one
    let mut res = Arena::<P, P>::new();
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
