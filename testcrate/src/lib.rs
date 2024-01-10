use std::path::PathBuf;

use starlight::{Epoch, EvalError};

pub fn _render(epoch: &Epoch) -> Result<(), EvalError> {
    epoch.render_to_svgs_in_dir(PathBuf::from("./".to_owned()))
}
