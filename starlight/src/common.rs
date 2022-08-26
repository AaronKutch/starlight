use triple_arena::ptr_struct;

#[cfg(debug_assertions)]
ptr_struct!(PLut; PBit);

#[cfg(not(debug_assertions))]
ptr_struct!(PLut(); PBit());
