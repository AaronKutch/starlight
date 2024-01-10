use std::path::PathBuf;

use starlight::{Epoch, Error};

pub fn _render(epoch: &Epoch) -> Result<(), Error> {
    epoch.render_to_svgs_in_dir(PathBuf::from("./".to_owned()))
}
