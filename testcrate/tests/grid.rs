use starlight::misc::Grid;

// copied from unit test that we do not want to format
#[test]
fn grid() {
    let grid: Grid<u64> = Grid::try_from([[0, 1, 2], [3, 4, 5], [6, 7, 8]]).unwrap();

    let expected_pairs = [
        (0, 1),
        (1, 2),
        (0, 3),
        (3, 4),
        (1, 4),
        (4, 5),
        (2, 5),
        (3, 6),
        (6, 7),
        (4, 7),
        (7, 8),
        (5, 8),
    ];
    let mut encountered = vec![];
    grid.for_each_orthogonal_pair(|t0, _, t1, _| encountered.push((*t0, *t1)));
    assert_eq!(expected_pairs.as_slice(), encountered.as_slice());
}
