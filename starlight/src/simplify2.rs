/*
impl Simplifier {
    fn find_single_node_simplifications(&mut self, p: PTNode) {

        let removed = self.a.remove(p).unwrap();
        for inp in &removed.inp {
            for (i, out) in self.a[inp].out.iter().enumerate() {
                if *out == p {
                    self.a[inp].out.swap_remove(i);
                    break
                }
            }
        }
        for out in &removed.out {
            for (i, inp) in self.a[out].inp.iter().enumerate() {
                if *inp == p {
                    self.a[out].inp.swap_remove(i);
                    break
                }
            }
        }
    }

}
*/